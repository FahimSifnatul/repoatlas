(class_declaration name: (identifier) @name) @class
(object_declaration name: (identifier) @name) @class
(function_declaration name: (identifier) @name) @func
(call_expression . (identifier) @callee) @call
(call_expression . (navigation_expression (identifier) @callee .)) @call
(value_argument (identifier) @ref)
[(line_comment) (block_comment)] @comment
(string_literal) @string
