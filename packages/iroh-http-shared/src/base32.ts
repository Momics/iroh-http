/**
 * Inline RFC 4648 base32 codec (a-z2-7, no padding, lowercase).
 *
 * Iroh node IDs use this encoding. Implemented inline to avoid any
 * external dependency.
 */

const B32_ALPHA = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const B32_LOOKUP = new Uint8Array(256).fill(0xff);
for (let i = 0; i < B32_ALPHA.length; i++) {
  B32_LOOKUP[B32_ALPHA.charCodeAt(i)] = i;
}

/** Encode raw bytes to a lowercase base32 string (no padding). */
export const base32Encode = (b: Uint8Array): string => {
  let out = "", buf = 0, bits = 0;
  for (const byte of b) {
    buf = (buf << 8) | byte;
    bits += 8;
    while (bits >= 5) {
      bits -= 5;
      out += B32_ALPHA[(buf >> bits) & 0x1f];
    }
  }
  if (bits > 0) out += B32_ALPHA[(buf << (5 - bits)) & 0x1f];
  return out.toLowerCase();
};

/** Decode a base32 string (case-insensitive) to raw bytes. */
export const base32Decode = (s: string): Uint8Array => {
  const out: number[] = [];
  let buf = 0, bits = 0;
  for (const ch of s.toUpperCase()) {
    if (ch === "=") break;
    const val = B32_LOOKUP[ch.charCodeAt(0)];
    if (val === 0xff) throw new Error(`Invalid base32 character: ${ch}`);
    buf = (buf << 5) | val;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((buf >> bits) & 0xff);
    }
  }
  return new Uint8Array(out);
};
