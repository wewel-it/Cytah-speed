use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tokio::time::interval;
use tracing::{info, warn, error, debug};

/// DoS protection configuration
#[derive(Debug, Clone)]
pub struct DosProtectionConfig {
    /// Maximum connections per IP
    pub max_connections_per_ip: u32,
    /// Maximum requests per second per IP
    pub max_requests_per_second: u32,
    /// Maximum bandwidth per second per IP (bytes)
    pub max_bandwidth_per_second: u64,
    /// Ban duration for violations
    pub ban_duration_seconds: u64,
    /// Cleanup interval for old entries
    pub cleanup_interval_seconds: u64,
    /// Maximum tracked IPs
    pub max_tracked_ips: usize,
}

impl Default for DosProtectionConfig {
    fn default() -> Self {
        Self {
            max_connections_per_ip: 10,
            max_requests_per_second: 100,
            max_bandwidth_per_second: 10 * 1024 * 1024, // 10MB/s
            ban_duration_seconds: 3600, // 1 hour
            cleanup_interval_seconds: 300, // 5 minutes
            max_tracked_ips: 10000,
        }
    }
}

/// Peer connection state
#[derive(Debug, Clone)]
struct PeerState {
    /// Current active connections
    connections: u32,
    /// Request timestamps (for rate limiting)
    request_times: Vec<Instant>,
    /// Bytes sent in current window
    bytes_sent: u64,
    /// Bytes received in current window
    bytes_received: u64,
    /// Last activity timestamp
    last_activity: Instant,
    /// Ban status
    banned_until: Option<Instant>,
    /// Violation count
    violations: u32,
}

/// DoS protection system
pub struct DosProtection {
    /// Configuration
    config: DosProtectionConfig,
    /// Peer states by IP
    peer_states: Arc<RwLock<HashMap<IpAddr, PeerState>>>,
    /// Statistics
    stats: Arc<RwLock<DosStats>>,
}

impl DosProtection {
    /// Create new DoS protection system
    pub fn new(config: DosProtectionConfig) -> Self {
        let protection = Self {
            config: config.clone(),
            peer_states: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(DosStats::default())),
        };

        // Start cleanup task
        protection.start_cleanup_task();

        protection
    }

    /// Check if connection from IP is allowed
    pub fn check_connection_allowed(&self, ip: IpAddr) -> Result<(), DosError> {
        let mut states = self.peer_states.write();
        let now = Instant::now();

        let state = states.entry(ip).or_insert_with(|| PeerState {
            connections: 0,
            request_times: Vec::new(),
            bytes_sent: 0,
            bytes_received: 0,
            last_activity: now,
            banned_until: None,
            violations: 0,
        });

        // Check if banned
        if let Some(ban_until) = state.banned_until {
            if now < ban_until {
                let mut stats = self.stats.write();
                stats.blocked_connections += 1;
                return Err(DosError::Banned {
                    remaining_seconds: (ban_until - now).as_secs(),
                });
            } else {
                // Ban expired, reset
                state.banned_until = None;
                state.violations = 0;
            }
        }

        // Check connection limit
        if state.connections >= self.config.max_connections_per_ip {
            self.handle_violation(ip, state, "connection limit exceeded");
            return Err(DosError::ConnectionLimitExceeded);
        }

        state.connections += 1;
        state.last_activity = now;

        Ok(())
    }

    /// Check if request from IP is allowed
    pub fn check_request_allowed(&self, ip: IpAddr) -> Result<(), DosError> {
        let mut states = self.peer_states.write();
        let now = Instant::now();

        let state = states.entry(ip).or_insert_with(|| PeerState {
            connections: 0,
            request_times: Vec::new(),
            bytes_sent: 0,
            bytes_received: 0,
            last_activity: now,
            banned_until: None,
            violations: 0,
        });

        // Check if banned
        if let Some(ban_until) = state.banned_until {
            if now < ban_until {
                let mut stats = self.stats.write();
                stats.blocked_requests += 1;
                return Err(DosError::Banned {
                    remaining_seconds: (ban_until - now).as_secs(),
                });
            }
        }

        // Clean old request times (older than 1 second)
        state.request_times.retain(|&time| now.duration_since(time) < Duration::from_secs(1));

        // Check request rate
        if state.request_times.len() >= self.config.max_requests_per_second as usize {
            self.handle_violation(ip, state, "request rate exceeded");
            return Err(DosError::RateLimitExceeded);
        }

        state.request_times.push(now);
        state.last_activity = now;

        Ok(())
    }

    /// Record bandwidth usage
    pub fn record_bandwidth(&self, ip: IpAddr, bytes_sent: u64, bytes_received: u64) {
        let mut states = self.peer_states.write();
        let now = Instant::now();

        let state = states.entry(ip).or_insert_with(|| PeerState {
            connections: 0,
            request_times: Vec::new(),
            bytes_sent: 0,
            bytes_received: 0,
            last_activity: now,
            banned_until: None,
            violations: 0,
        });

        state.bytes_sent += bytes_sent;
        state.bytes_received += bytes_received;
        state.last_activity = now;

        // Check bandwidth limits
        let total_bandwidth = state.bytes_sent + state.bytes_received;
        if total_bandwidth > self.config.max_bandwidth_per_second {
            self.handle_violation(ip, state, "bandwidth limit exceeded");
        }
    }

    /// Remove connection from IP
    pub fn remove_connection(&self, ip: IpAddr) {
        let mut states = self.peer_states.write();
        if let Some(state) = states.get_mut(&ip) {
            if state.connections > 0 {
                state.connections -= 1;
            }
        }
    }

    /// Handle a violation by increasing violation count and potentially banning
    fn handle_violation(
        &self,
        ip: IpAddr,
        state: &mut PeerState,
        reason: &str,
    ) {
        state.violations += 1;

        let mut stats = self.stats.write();
        stats.total_violations += 1;

        warn!("DoS violation from {}: {} (violation #{})", ip, reason, state.violations);

        // Ban after 3 violations
        if state.violations >= 3 {
            let ban_duration = Duration::from_secs(self.config.ban_duration_seconds);
            state.banned_until = Some(Instant::now() + ban_duration);
            stats.bans_issued += 1;

            warn!("Banned {} for {} seconds due to repeated violations", ip, self.config.ban_duration_seconds);
        }
    }

    /// Start periodic cleanup task
    fn start_cleanup_task(&self) {
        let states = Arc::clone(&self.peer_states);
        let config = self.config.clone();
        let stats = Arc::clone(&self.stats);

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(config.cleanup_interval_seconds));

            loop {
                interval.tick().await;

                let mut states_write = states.write();
                let now = Instant::now();
                let mut removed = 0;

                // Remove old entries
                states_write.retain(|ip, state| {
                    let age = now.duration_since(state.last_activity);
                    if age > Duration::from_secs(config.cleanup_interval_seconds * 2) {
                        removed += 1;
                        false
                    } else {
                        // Reset bandwidth counters periodically
                        if age > Duration::from_secs(1) {
                            state.bytes_sent = 0;
                            state.bytes_received = 0;
                        }
                        true
                    }
                });

                // Limit total tracked IPs
                if states_write.len() > config.max_tracked_ips {
                    let to_remove = states_write.len() - config.max_tracked_ips;
                    let mut entries: Vec<(IpAddr, Instant)> = states_write.iter().map(|(ip, state)| (*ip, state.last_activity)).collect();
                    entries.sort_by(|a, b| a.1.cmp(&b.1));

                    for (ip, _) in entries.into_iter().take(to_remove) {
                        states_write.remove(&ip);
                        removed += 1;
                    }
                }

                if removed > 0 {
                    debug!("Cleaned up {} old peer entries", removed);
                }

                // Update stats
                let mut stats_write = stats.write();
                stats_write.tracked_ips = states_write.len();
            }
        });
    }

    /// Get current statistics
    pub fn get_stats(&self) -> DosStats {
        self.stats.read().clone()
    }

    /// Manually unban an IP
    pub fn unban_ip(&self, ip: IpAddr) {
        let mut states = self.peer_states.write();
        if let Some(state) = states.get_mut(&ip) {
            state.banned_until = None;
            state.violations = 0;
            info!("Manually unbanned IP: {}", ip);
        }
    }

    /// Check if IP is currently banned
    pub fn is_banned(&self, ip: IpAddr) -> bool {
        let states = self.peer_states.read();
        if let Some(state) = states.get(&ip) {
            if let Some(ban_until) = state.banned_until {
                return Instant::now() < ban_until;
            }
        }
        false
    }
}

/// DoS protection errors
#[derive(Debug, thiserror::Error)]
pub enum DosError {
    #[error("IP address is banned for {remaining_seconds} more seconds")]
    Banned { remaining_seconds: u64 },

    #[error("Connection limit exceeded")]
    ConnectionLimitExceeded,

    #[error("Request rate limit exceeded")]
    RateLimitExceeded,

    #[error("Bandwidth limit exceeded")]
    BandwidthLimitExceeded,
}

/// DoS protection statistics
#[derive(Debug, Clone, Default)]
pub struct DosStats {
    pub tracked_ips: usize,
    pub total_violations: u64,
    pub bans_issued: u64,
    pub blocked_connections: u64,
    pub blocked_requests: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn test_connection_limits() {
        let config = DosProtectionConfig {
            max_connections_per_ip: 2,
            ..Default::default()
        };
        let dos = DosProtection::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // First connection should be allowed
        assert!(dos.check_connection_allowed(ip).is_ok());

        // Second connection should be allowed
        assert!(dos.check_connection_allowed(ip).is_ok());

        // Third connection should be blocked
        assert!(matches!(dos.check_connection_allowed(ip), Err(DosError::ConnectionLimitExceeded)));
    }

    #[tokio::test]
    async fn test_request_rate_limiting() {
        let config = DosProtectionConfig {
            max_requests_per_second: 2,
            ..Default::default()
        };
        let dos = DosProtection::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // First request should be allowed
        assert!(dos.check_request_allowed(ip).is_ok());

        // Second request should be allowed
        assert!(dos.check_request_allowed(ip).is_ok());

        // Third request should be blocked
        assert!(matches!(dos.check_request_allowed(ip), Err(DosError::RateLimitExceeded)));
    }

    #[test]
    fn test_ban_mechanism() {
        let config = DosProtectionConfig {
            max_connections_per_ip: 1,
            ..Default::default()
        };
        let dos = DosProtection::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // Cause violations
        for _ in 0..3 {
            let _ = dos.check_connection_allowed(ip);
        }

        // Should be banned after 3 violations
        assert!(dos.is_banned(ip));
    }

    #[test]
    fn test_unban() {
        let config = DosProtectionConfig {
            max_connections_per_ip: 1,
            ..Default::default()
        };
        let dos = DosProtection::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // Cause violations to get banned
        for _ in 0..3 {
            let _ = dos.check_connection_allowed(ip);
        }
        assert!(dos.is_banned(ip));

        // Unban
        dos.unban_ip(ip);
        assert!(!dos.is_banned(ip));
    }
}