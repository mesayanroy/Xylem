import * as StellarSdk from 'stellar-sdk';

const HORIZON_URL = process.env.NEXT_PUBLIC_HORIZON_URL || 'https://horizon-testnet.stellar.org';
const NETWORK_PASSPHRASE = process.env.NEXT_PUBLIC_STELLAR_NETWORK === 'mainnet'
  ? StellarSdk.Networks.PUBLIC
  : StellarSdk.Networks.TESTNET;

export const server = new StellarSdk.Horizon.Server(HORIZON_URL);

export { NETWORK_PASSPHRASE };

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
