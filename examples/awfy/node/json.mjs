export const warmup = 20, iters = 100, size = 1000, expected = 25280;

// Fixed input exercising objects, arrays, strings, integers, bools, null.
// The identical literal is embedded in all three languages.
export const INPUT =
  '{"widget":{"debug":"on","window":{"title":"Sample","width":500,"height":300},"items":[1,2,3,4,5],"enabled":true,"data":null}}';

// A tiny hand-written recursive-descent parser (no JSON.parse; the point is
// comparable parser codegen). Values: {null,bool,num,str,arr,obj}.
function parse(s) {
  let i = 0;
  const skipWs = () => { while (i < s.length && (s[i] === ' ' || s[i] === '\n' || s[i] === '\t' || s[i] === '\r')) i++; };
  function value() {
    skipWs();
    const c = s[i];
    if (c === '{') return obj();
    if (c === '[') return arr();
    if (c === '"') return { t: "str", v: str() };
    if (c === 't') { i += 4; return { t: "bool", v: true }; }
    if (c === 'f') { i += 5; return { t: "bool", v: false }; }
    if (c === 'n') { i += 4; return { t: "null" }; }
    return { t: "num", v: num() };
  }
  function str() {
    i++; // opening quote
    let start = i;
    while (s[i] !== '"') i++;
    const out = s.slice(start, i);
    i++; // closing quote
    return out;
  }
  function num() {
    let start = i;
    if (s[i] === '-') i++;
    while (i < s.length && s[i] >= '0' && s[i] <= '9') i++;
    return parseInt(s.slice(start, i), 10);
  }
  function arr() {
    i++; // [
    const items = [];
    skipWs();
    if (s[i] === ']') { i++; return { t: "arr", v: items }; }
    while (true) {
      items.push(value());
      skipWs();
      if (s[i] === ',') { i++; continue; }
      i++; // ]
      break;
    }
    return { t: "arr", v: items };
  }
  function obj() {
    i++; // {
    const entries = [];
    skipWs();
    if (s[i] === '}') { i++; return { t: "obj", v: entries }; }
    while (true) {
      skipWs();
      const key = str();
      skipWs();
      i++; // :
      const v = value();
      entries.push({ key, value: v });
      skipWs();
      if (s[i] === ',') { i++; continue; }
      i++; // }
      break;
    }
    return { t: "obj", v: entries };
  }
  return value();
}

// Fold: count every parsed node and sum all integers.
function fold(node, acc) {
  acc.count++;
  switch (node.t) {
    case "num": acc.sum += node.v; break;
    case "arr": for (const x of node.v) fold(x, acc); break;
    case "obj": for (const e of node.v) fold(e.value, acc); break;
  }
  return acc;
}

export function run(size) {
  let checksum = 0;
  for (let r = 0; r < size; r++) {
    const tree = parse(INPUT);
    const acc = fold(tree, { sum: 0, count: 0 });
    checksum = acc.sum * 31 + acc.count;
  }
  return checksum;
}
