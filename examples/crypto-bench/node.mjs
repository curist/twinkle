import { createHash, createHmac } from "node:crypto";

function makeBytes(n) {
  const b = Buffer.allocUnsafe(n);
  for (let i = 0; i < n; i++) b[i] = i & 0xff;
  return b;
}

function ms() { return performance.now(); }
function print(bench, iters, elapsed, sink) {
  console.log(`node\t${bench}\t${iters}\t${elapsed}\t${sink}`);
}

const small = Buffer.from("The quick brown fox jumps over the lazy dog");
const large = makeBytes(4096);
const key = Buffer.from("key");

const itersSmall = 5001;
const itersLarge = 501;
const itersHmac = 3001;
const itersB64 = 501;

let sink, start;

sink = 0; start = ms();
for (let i = 0; i < itersSmall; i++) sink ^= createHash("md5").update(small).digest()[0];
print("md5_small", itersSmall, ms() - start, sink);

sink = 0; start = ms();
for (let i = 0; i < itersSmall; i++) sink ^= createHash("sha1").update(small).digest()[0];
print("sha1_small", itersSmall, ms() - start, sink);

sink = 0; start = ms();
for (let i = 0; i < itersSmall; i++) sink ^= createHash("sha256").update(small).digest()[0];
print("sha256_small", itersSmall, ms() - start, sink);

sink = 0; start = ms();
for (let i = 0; i < itersHmac; i++) sink ^= createHmac("sha256", key).update(small).digest()[0];
print("hmac_sha256_small", itersHmac, ms() - start, sink);

sink = 0; start = ms();
for (let i = 0; i < itersLarge; i++) sink ^= createHash("md5").update(large).digest()[0];
print("md5_4k", itersLarge, ms() - start, sink);

sink = 0; start = ms();
for (let i = 0; i < itersLarge; i++) sink ^= createHash("sha1").update(large).digest()[0];
print("sha1_4k", itersLarge, ms() - start, sink);

sink = 0; start = ms();
for (let i = 0; i < itersLarge; i++) sink ^= createHash("sha256").update(large).digest()[0];
print("sha256_4k", itersLarge, ms() - start, sink);

sink = 0; start = ms();
for (let i = 0; i < itersB64; i++) {
  const text = large.toString("base64");
  const bytes = Buffer.from(text, "base64");
  sink ^= bytes[0] ^ bytes[bytes.length - 1];
}
print("base64_roundtrip_4k", itersB64, ms() - start, sink);
