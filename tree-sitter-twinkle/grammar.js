/**
 * @file Twinkle grammar for tree-sitter
 * @author curist <curist.cyc@gmail.com>
 * @license MIT
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

module.exports = grammar({
  name: "twinkle",

  externals: $ => [],

  conflicts: $ => [],

  extras: $ => [
    /[ \t\r\n]/,
    $.comment,
  ],

  word: $ => $.identifier,

  rules: {
    // ===== Program Structure =====

    source_file: $ => repeat(seq(
      $._top_level,
      optional($._terminator),
    )),

    _top_level: $ => choice(
      $.use_declaration,
      $.type_declaration,
      $.function_declaration,
      $.extern_declaration,
      $.extern_block,
      $._top_level_statement,
    ),

    use_declaration: $ => seq(
      'use',
      field('path', $.module_path),
      optional(seq('as', field('alias', $.identifier))),
    ),

    import_item_list: $ => seq(
      '{',
      $.import_item,
      repeat(seq(',', $.import_item)),
      optional(','),
      '}',
    ),

    import_item: $ => seq(
      field('name', $.identifier),
      optional(seq('as', field('alias', $.identifier))),
    ),

    module_path: $ => prec.right(choice(
      // stdlib: @std.fs or @std.fs.{...}
      seq('@', $.identifier, repeat(seq('.', $.identifier)), optional(seq('.', field('items', $.import_item_list)))),
      // relative: .helper, .sub.mod or .foo.{...}
      seq('.', $.identifier, repeat(seq('.', $.identifier)), optional(seq('.', field('items', $.import_item_list)))),
      // absolute: foo.bar, utils or foo.bar.{...}
      seq($.identifier, repeat(seq('.', $.identifier)), optional(seq('.', field('items', $.import_item_list)))),
    )),

    _top_level_statement: $ => choice(
      $.top_level_let_binding,
      $.for_statement,
      $._expression,
    ),

    top_level_let_binding: $ => seq(
      optional('pub'),
      $.let_binding,
    ),

    // Optional explicit semicolon separator. Newlines are whitespace (in extras)
    // and statement boundaries are determined by grammar structure — this allows
    // method-chain continuation across newlines (e.g. foo\n  .bar()).
    _terminator: $ => ';',

    // ===== Type Declarations =====

    type_declaration: $ => seq(
      optional('pub'),
      'type',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameters)),
      '=',
      field('definition', $._type_definition),
    ),

    _type_definition: $ => choice(
      $.record_type_def,
      $.sum_type_def,
      $.type,
    ),

    record_type_def: $ => seq(
      '.',
      '{',
      optional($.record_field_declarations),
      '}',
    ),

    record_field_declarations: $ => seq(
      $.record_field_declaration,
      repeat(seq(',', $.record_field_declaration)),
      optional(','),
    ),

    record_field_declaration: $ => seq(
      field('name', $.identifier),
      ':',
      field('type', $.type),
    ),

    sum_type_def: $ => seq(
      '{',
      $.variant_definition_list,
      '}',
    ),

    variant_definition_list: $ => seq(
      $.variant_definition,
      repeat(seq(',', $.variant_definition)),
      optional(','),
    ),

    variant_definition: $ => seq(
      field('name', $.identifier),
      optional(seq('(', $.type_list, ')')),
    ),

    // ===== Functions =====

    function_declaration: $ => seq(
      optional('pub'),
      'fn',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameters)),
      field('parameters', $.parameter_list),
      optional(field('return_type', $.type)),
      field('body', $.block),
    ),

    function_expression: $ => seq(
      'fn',
      field('parameters', $.parameter_list),
      optional(field('return_type', $.type)),
      field('body', $.block),
    ),

    extern_declaration: $ => prec.right(seq(
      optional('pub'),
      'extern',
      field('module', $.identifier),
      'fn',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(field('return_type', $.extern_type)),
    )),

    extern_block: $ => seq(
      optional('pub'),
      'extern',
      field('module', $.identifier),
      '{',
      repeat($.extern_signature),
      '}',
    ),

    extern_signature: $ => prec.right(seq(
      'fn',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(field('return_type', $.extern_type)),
    )),

    extern_type: $ => $._base_type,

    type_parameters: $ => seq(
      '<',
      $.type_parameter,
      repeat(seq(',', $.type_parameter)),
      optional(','),
      '>',
    ),

    type_parameter: $ => $.identifier,

    parameter_list: $ => seq(
      '(',
      optional($.parameters),
      ')',
    ),

    parameters: $ => seq(
      $.parameter,
      repeat(seq(',', $.parameter)),
      optional(','),
    ),

    parameter: $ => seq(
      field('name', $.identifier),
      optional(seq(':', field('type', $.type))),
    ),

    // ===== Expressions =====
    // Expressions are structured hierarchically by precedence

    _expression: $ => $.assignment_expression,

    assignment_expression: $ => choice(
      prec.right('assign', seq(
        field('left', $._lvalue),
        field('operator', '='),
        field('right', $._expression),
      )),
      $.range_expression,
    ),

    range_expression: $ => choice(
      prec.left('range', seq(
        field('start', $.logical_or_expression),
        field('operator', '..'),
        field('end', $.logical_or_expression),
      )),
      $.logical_or_expression,
    ),

    _lvalue: $ => choice(
      $.identifier,
      $.field_access,
      $.index_access,
    ),

    logical_or_expression: $ => choice(
      prec.left('or', seq(
        field('left', $.logical_or_expression),
        'or',
        field('right', $.logical_and_expression),
      )),
      $.logical_and_expression,
    ),

    logical_and_expression: $ => choice(
      prec.left('and', seq(
        field('left', $.logical_and_expression),
        'and',
        field('right', $.bitwise_or_expression),
      )),
      $.bitwise_or_expression,
    ),

    bitwise_or_expression: $ => choice(
      prec.left('bitwise_or', seq(
        field('left', $.bitwise_or_expression),
        field('operator', '|'),
        field('right', $.bitwise_xor_expression),
      )),
      $.bitwise_xor_expression,
    ),

    bitwise_xor_expression: $ => choice(
      prec.left('bitwise_xor', seq(
        field('left', $.bitwise_xor_expression),
        field('operator', '^'),
        field('right', $.bitwise_and_expression),
      )),
      $.bitwise_and_expression,
    ),

    bitwise_and_expression: $ => choice(
      prec.left('bitwise_and', seq(
        field('left', $.bitwise_and_expression),
        field('operator', '&'),
        field('right', $.equality_expression),
      )),
      $.equality_expression,
    ),

    equality_expression: $ => choice(
      prec.left('equality', seq(
        field('left', $.equality_expression),
        field('operator', choice('==', '!=')),
        field('right', $.comparison_expression),
      )),
      $.comparison_expression,
    ),

    comparison_expression: $ => choice(
      prec.left('comparison', seq(
        field('left', $.comparison_expression),
        field('operator', choice('<', '<=', '>', '>=')),
        field('right', $.shift_expression),
      )),
      $.shift_expression,
    ),

    shift_expression: $ => choice(
      prec.left('shift', seq(
        field('left', $.shift_expression),
        field('operator', choice('<<', '>>')),
        field('right', $.additive_expression),
      )),
      $.additive_expression,
    ),

    additive_expression: $ => choice(
      prec.left('additive', seq(
        field('left', $.additive_expression),
        field('operator', choice('+', '-')),
        field('right', $.multiplicative_expression),
      )),
      $.multiplicative_expression,
    ),

    multiplicative_expression: $ => choice(
      prec.left('multiplicative', seq(
        field('left', $.multiplicative_expression),
        field('operator', choice('*', '/', '%')),
        field('right', $.unary_expression),
      )),
      $.unary_expression,
    ),

    unary_expression: $ => choice(
      prec('unary', seq(
        field('operator', choice('-', '!', '~', 'try')),
        field('operand', $.unary_expression),
      )),
      $._postfix_expression,
    ),

    _postfix_expression: $ => choice(
      $.call_expression,
      $.field_access,
      $.index_access,
      $._primary_expression,
    ),

    call_expression: $ => prec.dynamic(1, prec('postfix', seq(
      field('function', $._postfix_expression),
      field('arguments', $.argument_list),
    ))),

    argument_list: $ => seq(
      '(',
      optional($.arguments),
      ')',
    ),

    arguments: $ => seq(
      $._expression,
      repeat(seq(',', $._expression)),
      optional(','),
    ),

    field_access: $ => prec('postfix', seq(
      field('object', $._postfix_expression),
      '.',
      field('field', $.identifier),
    )),

    index_access: $ => prec('postfix', seq(
      field('object', $._postfix_expression),
      '[',
      field('index', $._expression),
      ']',
    )),

    _primary_expression: $ => choice(
      $._literal,
      $.identifier,
      $.variant_expression,
      $.function_expression,
      $.block,
      $.if_expression,
      $.case_expression,
      $.collect_expression,
      $.parenthesized_expression,
    ),

    parenthesized_expression: $ => seq(
      '(',
      $._expression,
      ')',
    ),

    variant_expression: $ => prec.dynamic(-1, seq(
      '.',
      field('variant', alias(token(/[A-Z][a-zA-Z0-9_]*/), $.identifier)),
      optional(seq('(', $.arguments, ')')),
    )),

    // ===== Pattern Matching =====

    case_expression: $ => seq(
      'case',
      field('value', $._expression),
      '{',
      $.case_arms,
      '}',
    ),

    case_arms: $ => seq(
      $.case_arm,
      repeat(seq(',', $.case_arm)),
      optional(','),
    ),

    case_arm: $ => seq(
      field('pattern', $._pattern),
      '=>',
      field('value', choice($.break_statement, $.continue_statement, $._expression)),
    ),

    _pattern: $ => choice(
      $.enum_pattern,
      $.wildcard_pattern,
      $.literal_pattern,
      $.identifier_pattern,
    ),

    enum_pattern: $ => choice(
      $.shorthand_enum_pattern,
      $.qualified_enum_pattern,
    ),

    shorthand_enum_pattern: $ => seq(
      '.',
      field('variant', $.identifier),
      optional(seq('(', $.pattern_list, ')')),
    ),

    qualified_enum_pattern: $ => seq(
      field('type', $.type_name),
      '.',
      field('variant', $.identifier),
      optional(seq('(', $.pattern_list, ')')),
    ),

    pattern_list: $ => seq(
      $._pattern,
      repeat(seq(',', $._pattern)),
      optional(','),
    ),

    wildcard_pattern: $ => '_',

    literal_pattern: $ => choice(
      $.int_literal,
      $.float_literal,
      $.bool_literal,
      $.string_literal,
    ),

    identifier_pattern: $ => $.identifier,

    // ===== Control Flow =====

    if_expression: $ => seq(
      'if',
      field('condition', $._expression),
      field('consequence', $.block),
      optional(seq('else', field('alternative', choice($.if_expression, $.block)))),
    ),

    // ===== Loops =====

    for_statement: $ => seq(
      'for',
      field('condition', $._for_condition),
      field('body', $.block),
    ),

    _for_condition: $ => choice(
      $.for_in_condition,
      $.for_while_condition,
    ),

    for_in_condition: $ => seq(
      field('binding', choice($.identifier, $.wildcard_pattern)),
      optional(seq(',', field('index', $.identifier))),
      'in',
      field('iterable', $._expression),
    ),

    for_while_condition: $ => field('condition', $._expression),

    collect_expression: $ => seq(
      'collect',
      field('binding', choice($.identifier, $.wildcard_pattern)),
      'in',
      field('iterable', $._expression),
      field('body', $.block),
    ),

    // ===== Literals =====

    _literal: $ => choice(
      prec.dynamic(5, $.string_literal), // prefer strings so the lexer chooses them early
      $.int_literal,
      $.float_literal,
      $.bool_literal,
      $.record_literal,
      $.array_literal,
    ),

    record_literal: $ => choice(
      $.anonymous_record_literal,
      $.named_record_literal,
    ),

    anonymous_record_literal: $ => seq(
      '.',
      '{',
      optional($.record_fields),
      '}',
    ),

    named_record_literal: $ => prec.dynamic(1, seq(
      field('type', $.type_name),
      '.',
      '{',
      optional($.record_fields),
      '}',
    )),

    record_fields: $ => seq(
      $.record_field,
      repeat(seq(',', $.record_field)),
      optional(','),
    ),

    record_field: $ => seq(
      field('name', $.identifier),
      optional(seq(
        ':',
        field('value', $._expression),
      )),
    ),

    array_literal: $ => seq(
      '[',
      optional($.array_elements),
      ']',
    ),

    array_elements: $ => seq(
      $._expression,
      repeat(seq(',', $._expression)),
      optional(','),
    ),

    // String literals with interpolation support
    string_literal: $ => seq(
      '"',
      repeat(choice(
        $.string_content,
        $.escape_sequence,
        $.interpolation,
        token.immediate('$'),
      )),
      '"',
    ),

    string_content: $ => token.immediate(/[^\n"\\$]+/),

    escape_sequence: $ => token.immediate(seq(
      '\\',
      choice(
        '"',
        '\\',
        '$',
        'n',
        't',
        'r',
        'e',
        /x[0-9a-fA-F]{2}/,
        /u\{[0-9a-fA-F]{1,6}\}/,
      ),
    )),

    interpolation: $ => seq(
      token.immediate('${'),
      $._expression,
      token.immediate('}'),
    ),

    int_literal: $ => choice(
      /0x[0-9a-fA-F]+/,
      /\d+/,
    ),

    float_literal: $ => /\d+\.\d+/,

    bool_literal: $ => choice('true', 'false'),

    // ===== Blocks and Statements =====

    block: $ => seq(
      '{',
      // Statements separated by grammar structure; optional ';' allowed anywhere.
      repeat(seq($._statement, optional($._terminator))),
      '}',
    ),

    _statement: $ => choice(
      $.for_statement,
      $.let_binding,
      $.defer_statement,
      $.break_statement,
      $.continue_statement,
      $.return_statement,
      $._expression,
    ),

    let_binding: $ => choice(
      seq(
        field('name', $.identifier),
        ':=',
        field('value', $._expression),
      ),
      seq(
        field('name', $.identifier),
        ':',
        field('type', $.type),
        '=',
        field('value', $._expression),
      ),
    ),

    defer_statement: $ => seq(
      'defer',
      field('expression', $._expression),
    ),

    break_statement: $ => 'break',

    continue_statement: $ => 'continue',

    return_statement: $ => seq(
      'return',
      field('value', $._expression),
    ),

    // ===== Types =====

    // Type supports two postfix sugar operators (applied left-to-right):
    //   T?     → Option<T>
    //   T!E    → Result<T, E>
    //   T?!E   → Result<Option<T>, E>
    //   !E     → Result<Void, E>   (leading `!`, no success type)
    // Bare `!` or `T!` without an error type are not valid.
    type: $ => choice(
      // !E shorthand: Result<Void, E>
      seq(
        '!',
        field('error_type', $._base_type),
      ),
      // T, T?, T!E, T?!E
      seq(
        choice($.function_type, $._base_type),
        optional('?'),
        optional(seq('!', field('error_type', $._base_type))),
      ),
    ),

    // Make function_type right-associative:
    // fn(A) fn(B) C  ==> fn(A) (fn(B) C)
    function_type: $ => prec.right(seq(
      'fn',
      '(',
      optional($.type_list),
      ')',
      optional($.type),
    )),

    _base_type: $ => choice(
      $.primitive_type,
      $.generic_type,
      $.type_name,
    ),

    primitive_type: $ => choice(
      'Int',
      'Float',
      'Bool',
      'String',
      'Void',
    ),

    generic_type: $ => seq(
      field('name', $.type_name),
      '<',
      field('arguments', $.type_arguments),
      '>',
    ),

    type_arguments: $ => seq(
      $.type,
      repeat(seq(',', $.type)),
      optional(','),
    ),

    type_list: $ => seq(
      $.type,
      repeat(seq(',', $.type)),
      optional(','),
    ),

    type_name: $ => seq(
      $.identifier,
      repeat(seq('.', $.identifier)),
    ),

    // ===== Lexical Rules =====

    identifier: $ => /[a-zA-Z][a-zA-Z0-9_]*/,

    comment: $ => token(seq('//', /.*/)),
  },

  precedences: $ => [
    [
      'assign',
      'range',
      'or',
      'and',
      'bitwise_or',
      'bitwise_xor',
      'bitwise_and',
      'equality',
      'comparison',
      'shift',
      'additive',
      'multiplicative',
      'unary',
      'postfix',
    ],
  ],

  conflicts: $ => [
    // Expected conflicts for ambiguous syntax
    [$.variant_expression],
    [$.type_name],
    [$.type],
    // identifier '.' is ambiguous between field_access and type_name
    [$._primary_expression, $.type_name],
    // Binary precedence edge cases
    [$.shift_expression, $.additive_expression],
    // Postfix vs unary (e.g. -x.y, -f(), -a[0])
    [$.unary_expression, $.field_access],
    [$.unary_expression, $.call_expression],
    [$.unary_expression, $.index_access],
  ],
});

// Helper function for separated lists
function sep1(rule, separator) {
  return seq(rule, repeat(seq(separator, rule)));
}
