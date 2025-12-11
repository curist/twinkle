; Keywords
[
  "fn"
  "type"
  "import"
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

; Operators
[
  "="
  ":="
  "+="
  "-="
  "*="
  "/="
  "%="
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

; String interpolation delimiters
(interpolation
  "${" @punctuation.special
  "}" @punctuation.special)

; Comments
(comment) @comment

; Types
(primitive_type) @type.builtin

(type_name
  (identifier) @type)

(generic_type
  name: (type_name
    (identifier) @type))

; Function definitions
(function_declaration
  name: (identifier) @function)

(function_expression) @function

; Function calls
(call_expression
  function: (identifier) @function.call)

(call_expression
  function: (field_access
    field: (identifier) @function.method.call))

; Type declarations
(type_declaration
  name: (identifier) @type.definition)

; Parameters
(parameter
  name: (identifier) @variable.parameter)

(type_parameter) @type.parameter

; Variables
(let_binding
  name: (identifier) @variable)

(identifier) @variable

; Fields
(field_access
  field: (identifier) @property)

(record_field
  name: (identifier) @property)

(record_field_declaration
  name: (identifier) @property)

; Variants
(variant_expression
  variant: (identifier) @constructor)

(variant_definition
  name: (identifier) @constructor)

(shorthand_enum_pattern
  variant: (identifier) @constructor)

(qualified_enum_pattern
  variant: (identifier) @constructor)

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

; Special
"?" @operator

; Wildcard pattern
(wildcard_pattern) @variable.builtin
