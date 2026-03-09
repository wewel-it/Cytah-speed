/// Simple gas meter tracking usage against a limit
#[derive(Default, Debug, Clone)]
pub struct GasMeter {
    pub limit: u64,
    pub used: u64,
}

impl GasMeter {
    pub fn new(limit: u64) -> Self {
        GasMeter { limit, used: 0 }
    }

    pub fn charge(&mut self, amount: u64) -> Result<(), String> {
        if self.used + amount > self.limit {
            Err("Out of gas".into())
        } else {
            self.used += amount;
            Ok(())
        }
    }
}
