package main

// Fixed input exercising objects, arrays, strings, integers, bools, null.
// The identical literal is embedded in all three languages.
const jsonInput = `{"widget":{"debug":"on","window":{"title":"Sample","width":500,"height":300},"items":[1,2,3,4,5],"enabled":true,"data":null}}`

type jsonKind int

const (
	jNull jsonKind = iota
	jBool
	jNum
	jStr
	jArr
	jObj
)

type jsonEntry struct {
	key   string
	value *jsonValue
}

type jsonValue struct {
	kind    jsonKind
	num     int
	str     string
	arr     []*jsonValue
	obj     []jsonEntry
}

type jsonParser struct {
	s string
	i int
}

func (p *jsonParser) skipWs() {
	for p.i < len(p.s) {
		c := p.s[p.i]
		if c == ' ' || c == '\n' || c == '\t' || c == '\r' {
			p.i++
		} else {
			break
		}
	}
}

func (p *jsonParser) parseStr() string {
	p.i++ // opening quote
	start := p.i
	for p.s[p.i] != '"' {
		p.i++
	}
	out := p.s[start:p.i]
	p.i++ // closing quote
	return out
}

func (p *jsonParser) parseNum() int {
	neg := false
	if p.s[p.i] == '-' {
		neg = true
		p.i++
	}
	n := 0
	for p.i < len(p.s) && p.s[p.i] >= '0' && p.s[p.i] <= '9' {
		n = n*10 + int(p.s[p.i]-'0')
		p.i++
	}
	if neg {
		return -n
	}
	return n
}

func (p *jsonParser) parseValue() *jsonValue {
	p.skipWs()
	c := p.s[p.i]
	switch {
	case c == '{':
		return p.parseObj()
	case c == '[':
		return p.parseArr()
	case c == '"':
		return &jsonValue{kind: jStr, str: p.parseStr()}
	case c == 't':
		p.i += 4
		return &jsonValue{kind: jBool}
	case c == 'f':
		p.i += 5
		return &jsonValue{kind: jBool}
	case c == 'n':
		p.i += 4
		return &jsonValue{kind: jNull}
	default:
		return &jsonValue{kind: jNum, num: p.parseNum()}
	}
}

func (p *jsonParser) parseArr() *jsonValue {
	p.i++ // [
	items := []*jsonValue{}
	p.skipWs()
	if p.s[p.i] == ']' {
		p.i++
		return &jsonValue{kind: jArr, arr: items}
	}
	for {
		items = append(items, p.parseValue())
		p.skipWs()
		if p.s[p.i] == ',' {
			p.i++
			continue
		}
		p.i++ // ]
		break
	}
	return &jsonValue{kind: jArr, arr: items}
}

func (p *jsonParser) parseObj() *jsonValue {
	p.i++ // {
	entries := []jsonEntry{}
	p.skipWs()
	if p.s[p.i] == '}' {
		p.i++
		return &jsonValue{kind: jObj, obj: entries}
	}
	for {
		p.skipWs()
		key := p.parseStr()
		p.skipWs()
		p.i++ // :
		v := p.parseValue()
		entries = append(entries, jsonEntry{key: key, value: v})
		p.skipWs()
		if p.s[p.i] == ',' {
			p.i++
			continue
		}
		p.i++ // }
		break
	}
	return &jsonValue{kind: jObj, obj: entries}
}

func jsonFold(n *jsonValue, sum, count *int) {
	*count++
	switch n.kind {
	case jNum:
		*sum += n.num
	case jArr:
		for _, x := range n.arr {
			jsonFold(x, sum, count)
		}
	case jObj:
		for _, e := range n.obj {
			jsonFold(e.value, sum, count)
		}
	}
}

func jsonRun(size int) int {
	checksum := 0
	for r := 0; r < size; r++ {
		p := &jsonParser{s: jsonInput, i: 0}
		tree := p.parseValue()
		sum, count := 0, 0
		jsonFold(tree, &sum, &count)
		checksum = sum*31 + count
	}
	return checksum
}

var jsonBench = Bench{Name: "json", Warmup: 20, Iters: 100, Size: 1000, Expected: 25280, Run: jsonRun}
