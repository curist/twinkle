// Validates a linked wasm binary that exercises runtime-backed user exports.
// Run after: twk run boot/tests/wasm_binary_runtime_linked.tw
import { readFileSync } from "node:fs";

const bytes = readFileSync(new URL("./wasm_binary_runtime_linked.wasm", import.meta.url));

const stub = () => {};
const stubNull = () => null;

const { instance } = await WebAssembly.instantiate(bytes, {
  host: {
    f64_to_string: stubNull,
    print: stub,
    println: stub,
    eprint: stub,
    eprintln: stub,
    error: stub,
    parse_float: () => ({ value: 0.0, ok: 0 }),
  },
});

const expectI64 = (name, want) => {
  const got = instance.exports[name]();
  if (got !== want) {
    console.error(`FAIL: ${name}() returned ${got}, expected ${want}`);
    process.exit(1);
  }
};

expectI64("string_len_plus", 4n);
expectI64("vector_len_plus", 5n);
expectI64("dict_len_plus", 2n);
console.log("PASS: runtime-linked exports returned expected values");
