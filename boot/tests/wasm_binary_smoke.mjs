// Validates the wasm binary emitted by wasm_binary_smoke.tw.
// Run after:  twk run boot/tests/wasm_binary_smoke.tw
import { readFileSync } from "node:fs";

const bytes = readFileSync(new URL("./wasm_binary_smoke.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(bytes);
const result = instance.exports.answer();
if (result !== 42) {
  console.error(`FAIL: answer() returned ${result}, expected 42`);
  process.exit(1);
}
console.log(`PASS: answer() = ${result}`);
