(class_declaration name: [(type_identifier) @name (user_type (type_identifier) @name)]) @class
(protocol_declaration name: (type_identifier) @name) @type
(function_declaration name: (simple_identifier) @name) @func
(call_expression . (simple_identifier) @callee) @call
(call_expression . (navigation_expression suffix: (navigation_suffix suffix: (simple_identifier) @callee))) @call
(value_argument value: (simple_identifier) @ref)
[(comment) (multiline_comment)] @comment
[(line_string_literal) (multi_line_string_literal)] @string
