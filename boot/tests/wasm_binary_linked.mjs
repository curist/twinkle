// Validates the wasm binary emitted by wasm_binary_linked.tw.
// Run after:  twk run boot/tests/wasm_binary_linked.tw
import { readFileSync } from "node:fs";

const bytes = readFileSync(new URL("./wasm_binary_linked.wasm", import.meta.url));

const stub = () => {};
const stubBigInt = () => 0n;
const stubNull = () => null;

try {
  await WebAssembly.instantiate(bytes, {
    "host": {
      f64_to_string: stubNull,
      print: stub, println: stub, eprint: stub, eprintln: stub,
      error: stub,
      parse_float: () => ({ value: 0.0, ok: 0 }),
    },
  });
} catch (e) {
  console.error("FAIL: instantiation error:", e.message);
  process.exit(1);
}
console.log("PASS: linked module instantiates successfully");
