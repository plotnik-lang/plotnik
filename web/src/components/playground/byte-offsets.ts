const ASCII_ONLY = /^[\x00-\x7F]*$/;

export type ByteToUtf16 = (byte: number) => number;

export function byteToUtf16(text: string): ByteToUtf16 {
  if (ASCII_ONLY.test(text)) {
    return (byte) => byte;
  }

  let byteLen = 0;
  for (const ch of text) {
    const cp = ch.codePointAt(0) ?? 0;
    byteLen += cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
  }

  const table = new Uint32Array(byteLen + 1);
  let byte = 0;
  let unit = 0;
  for (const ch of text) {
    const cp = ch.codePointAt(0) ?? 0;
    const bytes = cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;

    for (let k = 0; k < bytes; k += 1) {
      table[byte + k] = unit;
    }
    byte += bytes;
    unit += cp < 0x10000 ? 1 : 2;
  }
  table[byteLen] = unit;

  return (offset) => table[Math.min(Math.max(offset, 0), byteLen)] ?? 0;
}
