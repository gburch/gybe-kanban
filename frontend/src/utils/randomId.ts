// Provide a resilient UUID generator that works even when `crypto.randomUUID`
// is unavailable (e.g., older browsers or sandboxed preview runtimes).
export function randomId(): string {
  const globalObj: typeof globalThis | undefined =
    typeof globalThis !== 'undefined' ? globalThis : undefined;
  const cryptoObj: Crypto | undefined = globalObj?.crypto ?? (globalObj as any)?.msCrypto;

  if (cryptoObj?.randomUUID) {
    try {
      return cryptoObj.randomUUID();
    } catch (error) {
      // Fall through to manual generation if the browser throws (Safari < 17.4).
    }
  }

  if (cryptoObj?.getRandomValues) {
    const bytes = new Uint8Array(16);
    cryptoObj.getRandomValues(bytes);

    // RFC 4122 variant 1 UUID formatting
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    const toHex = (n: number) => n.toString(16).padStart(2, '0');
    const hex = Array.from(bytes, toHex).join('');
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  }

  // Last resort: timestamp + random number. Not RFC compliant but unique enough for UI state.
  return `uuid-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}
