; Parse errors - highlight syntax errors
(ERROR) @error

; Generic identifier - FIRST, so specific captures can override
(identifier) @variable

; Keywords
[
  "fn"
  "type"
  "use"
  "as"
  "pub"
  "if"
  "else"
  "case"
  "for"
  "in"
  "collect"
  "try"
] @keyword

; Statement keywords
(break_statement) @keyword.control
(continue_statement) @keyword.control

(return_statement
  "return" @keyword.return)

(defer_statement
  "defer" @keyword.control)

; Operators
[
  "="
  ":="
  "+"
  "-"
  "*"
  "/"
  "%"
  "=="
  "!="
  "<"
  "<="
  ">"
  ">="
  "!"
  "and"
  "or"
] @operator

; Boolean literals
(bool_literal) @boolean

; Numbers
(int_literal) @number
(float_literal) @number.float

; Strings
(string_literal) @string
(string_content) @string

; Comments
(comment) @comment

; Types - higher priority for type contexts
(primitive_type) @type.builtin

(type_name
  (identifier) @type)

(generic_type
  name: (type_name
    (identifier) @type))

; Type declarations
(type_declaration
  name: (identifier) @type.definition)

; Type parameters
(type_parameter
  (identifier) @type.parameter)

; Function definitions - higher priority
(function_declaration
  name: (identifier) @function)

(function_expression) @function

; Parameters
(parameter
  name: (identifier) @variable.parameter)

; Let bindings
(let_binding
  name: (identifier) @variable)

; Fields
(field_access
  field: (identifier) @property)

(record_field
  name: (identifier) @property)

(record_field_declaration
  name: (identifier) @property)

; Function calls - after fields so method calls take priority over property
(call_expression
  function: (identifier) @function.call)

(call_expression
  function: (field_access
    field: (identifier) @function.method.call))

; Variants - high priority for constructors
(variant_expression
  variant: (identifier) @constructor)

(variant_definition
  name: (identifier) @constructor)

(shorthand_enum_pattern
  variant: (identifier) @constructor)

(qualified_enum_pattern
  variant: (identifier) @constructor)

; Wildcard pattern
(wildcard_pattern) @variable.builtin

; Punctuation
[
  "("
  ")"
  "{"
  "}"
  "["
  "]"
] @punctuation.bracket

[
  ":"
  "."
  ","
  ";"
  "=>"
] @punctuation.delimiter

; String interpolation delimiters - after brackets so } takes priority over punctuation.bracket
(interpolation
  "${" @punctuation.special
  "}" @punctuation.special)

; Special
"?" @operator
