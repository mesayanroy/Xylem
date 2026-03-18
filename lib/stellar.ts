import * as StellarSdk from 'stellar-sdk';

const HORIZON_URL = process.env.NEXT_PUBLIC_HORIZON_URL || 'https://horizon-testnet.stellar.org';
const NETWORK_PASSPHRASE = process.env.NEXT_PUBLIC_STELLAR_NETWORK === 'mainnet'
  ? StellarSdk.Networks.PUBLIC
  : StellarSdk.Networks.TESTNET;

export const server = new StellarSdk.Horizon.Server(HORIZON_URL);

export { NETWORK_PASSPHRASE };

// ─── Horizon SSE watcher ──────────────────────────────────────────────────────

export type TxWatcherCallback = (tx: StellarSdk.Horizon.ServerApi.TransactionRecord) => void;

/**
 * Open a Horizon Server-Sent Events stream for a specific account and call
 * `onTransaction` for every new transaction that arrives.
 *
 * Returns a `close` function that stops the stream.
 *
 * @param accountAddress  Stellar account to watch.
 * @param onTransaction   Called with each new transaction record.
 * @param cursor          Starting cursor (default: "now" to only get new txs).
 */
export function watchAccountTransactions(
  accountAddress: string,
  onTransaction: TxWatcherCallback,
  cursor = 'now'
): () => void {
  const close = server
    .transactions()
    .forAccount(accountAddress)
    .cursor(cursor)
    .stream({
      onmessage: (tx) => {
        try {
          onTransaction(tx as StellarSdk.Horizon.ServerApi.TransactionRecord);
        } catch (err) {
          console.error('[Horizon SSE] onmessage handler error:', err);
        }
      },
      onerror: (err) => {
        console.error('[Horizon SSE] stream error:', err);
      },
    });

  return close;
}

/**
 * Poll Horizon for a specific transaction hash and resolve once it appears
 * on the ledger (or reject after `timeoutMs`).
 */
export function waitForTransaction(
  txHash: string,
  timeoutMs = 60_000
): Promise<StellarSdk.Horizon.ServerApi.TransactionRecord> {
  return new Promise((resolve, reject) => {
    const deadline = Date.now() + timeoutMs;

    async function poll(): Promise<void> {
      try {
        const tx = await server.transactions().transaction(txHash).call();
        if (tx) {
          resolve(tx);
          return;
        }
      } catch {
        // not yet on ledger
      }

      if (Date.now() >= deadline) {
        reject(new Error(`Transaction ${txHash} not found within ${timeoutMs}ms`));
        return;
      }

      setTimeout(() => void poll(), 3_000);
    }

    void poll();
  });
}

export function truncateAddress(address: string, chars = 4): string {
  if (!address || address.length <= chars * 2 + 3) return address;
  return `${address.slice(0, chars + 1)}...${address.slice(-chars)}`;
}

export async function verifyPaymentTransaction(
  txHash: string,
  expectedDestination: string,
  expectedAmountXlm: number,
  expectedMemo: string
): Promise<{ valid: boolean; error?: string }> {
  try {
    const tx = await server.transactions().transaction(txHash).call();
    if (!tx) return { valid: false, error: 'Transaction not found' };
    // Memo check: the expected value may be a prefix of the actual memo
    // (e.g. "agent:<id>" matches "agent:<id>:req:<nonce>")
    if (expectedMemo && tx.memo && !tx.memo.startsWith(expectedMemo)) {
      return { valid: false, error: 'Memo mismatch' };
    }

    const ops = await server.operations().forTransaction(txHash).call();
    let totalPaid = 0;

    for (const op of ops.records) {
      if (
        op.type === 'payment' &&
        (op as StellarSdk.Horizon.ServerApi.PaymentOperationRecord).asset_type === 'native' &&
        (op as StellarSdk.Horizon.ServerApi.PaymentOperationRecord).to === expectedDestination
      ) {
        totalPaid += parseFloat(
          (op as StellarSdk.Horizon.ServerApi.PaymentOperationRecord).amount
        );
      }
    }

    if (totalPaid < expectedAmountXlm) {
      return {
        valid: false,
        error: `Payment amount ${totalPaid} XLM is less than required ${expectedAmountXlm} XLM`,
      };
    }

    return { valid: true };
  } catch (err) {
    return { valid: false, error: `Verification failed: ${String(err)}` };
  }
}

export async function getXlmBalance(address: string): Promise<string> {
  try {
    const account = await server.loadAccount(address);
    const xlmBalance = account.balances.find(
      (b) => b.asset_type === 'native'
    );
    return xlmBalance ? xlmBalance.balance : '0';
  } catch {
    return '0';
  }
}

export async function fundTestAccount(address: string): Promise<boolean> {
  try {
    const res = await fetch(`https://friendbot.stellar.org?addr=${encodeURIComponent(address)}`);
    return res.ok;
  } catch {
    return false;
  }
}
