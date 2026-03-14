/**
 * Cytah-Speed Wallet Bridge
 * Browser wallet integration for dApps
 */

class CytahWalletBridge {
    constructor(provider) {
        this.provider = provider;
        this.connectedWallet = null;
        this.accounts = [];
    }

    /**
     * Connect to a browser wallet extension
     */
    async connectWallet() {
        if (typeof window === 'undefined') {
            throw new Error('Wallet bridge only works in browser environment');
        }

        // Check for Cytah-Speed wallet extension
        if (!window.cytahWallet) {
            throw new Error('Cytah-Speed wallet extension not found. Please install the Cytah-Speed wallet.');
        }

        try {
            // Request wallet connection
            const result = await window.cytahWallet.connect();
            this.connectedWallet = window.cytahWallet;
            this.accounts = result.accounts || [];

            return {
                success: true,
                accounts: this.accounts,
                wallet: result.wallet
            };
        } catch (error) {
            throw new Error(`Failed to connect wallet: ${error.message}`);
        }
    }

    /**
     * Disconnect from wallet
     */
    async disconnectWallet() {
        if (this.connectedWallet) {
            await this.connectedWallet.disconnect();
            this.connectedWallet = null;
            this.accounts = [];
        }
    }

    /**
     * Get connected accounts
     */
    getAccounts() {
        return this.accounts;
    }

    /**
     * Sign a transaction using connected wallet
     */
    async signTransaction(tx) {
        if (!this.connectedWallet) {
            throw new Error('No wallet connected');
        }

        return await this.connectedWallet.signTransaction(tx);
    }

    /**
     * Send a signed transaction
     */
    async sendTransaction(tx) {
        if (!this.connectedWallet) {
            throw new Error('No wallet connected');
        }

        const signedTx = await this.signTransaction(tx);
        return await this.provider.sendTransaction(signedTx);
    }

    /**
     * Sign and send a transaction in one step
     */
    async signAndSendTransaction(tx) {
        const signedTx = await this.signTransaction(tx);
        return await this.provider.sendTransaction(signedTx);
    }

    /**
     * Get wallet balance
     */
    async getBalance(address = null) {
        const targetAddress = address || (this.accounts.length > 0 ? this.accounts[0] : null);
        if (!targetAddress) {
            throw new Error('No address available');
        }

        return await this.provider.getBalance(targetAddress);
    }

    /**
     * Deploy contract using connected wallet
     */
    async deployContract(wasmCode, initArgs = null) {
        if (!this.connectedWallet) {
            throw new Error('No wallet connected');
        }

        const signedTx = await this.connectedWallet.signContractDeployment(wasmCode, initArgs);
        return await this.provider.sendTransaction(signedTx);
    }

    /**
     * Call contract using connected wallet
     */
    async callContract(contractAddress, method, args = null) {
        if (!this.connectedWallet) {
            throw new Error('No wallet connected');
        }

        const signedTx = await this.connectedWallet.signContractCall(contractAddress, method, args);
        return await this.provider.sendTransaction(signedTx);
    }

    /**
     * Check if wallet is connected
     */
    isConnected() {
        return this.connectedWallet !== null;
    }

    /**
     * Get wallet information
     */
    getWalletInfo() {
        if (!this.connectedWallet) {
            return null;
        }

        return {
            name: this.connectedWallet.name || 'Unknown',
            version: this.connectedWallet.version || 'Unknown',
            accounts: this.accounts,
            connected: true
        };
    }

    /**
     * Listen for wallet events
     */
    onWalletEvent(eventType, callback) {
        if (!this.connectedWallet) {
            throw new Error('No wallet connected');
        }

        return this.connectedWallet.on(eventType, callback);
    }

    /**
     * Request wallet permissions
     */
    async requestPermissions(permissions) {
        if (!this.connectedWallet) {
            throw new Error('No wallet connected');
        }

        return await this.connectedWallet.requestPermissions(permissions);
    }
}

// Global wallet bridge instance
let globalWalletBridge = null;

/**
 * Get or create global wallet bridge instance
 */
function getWalletBridge(provider = null) {
    if (!globalWalletBridge) {
        if (!provider) {
            throw new Error('Provider required for first wallet bridge creation');
        }
        globalWalletBridge = new CytahWalletBridge(provider);
    }
    return globalWalletBridge;
}

// Export for different environments
if (typeof module !== 'undefined' && module.exports) {
    // Node.js
    module.exports = { CytahWalletBridge, getWalletBridge };
} else if (typeof define === 'function' && define.amd) {
    // AMD
    define([], function() { return { CytahWalletBridge, getWalletBridge }; });
} else if (typeof window !== 'undefined') {
    // Browser global
    window.CytahWalletBridge = CytahWalletBridge;
    window.getCytahWalletBridge = getWalletBridge;
}