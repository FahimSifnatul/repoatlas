(class_definition name: (identifier) @name) @class
(object_definition name: (identifier) @name) @class
(trait_definition name: (identifier) @name) @class
(function_definition name: (identifier) @name) @func
(call_expression function: [(identifier) @callee (field_expression field: (identifier) @callee)]) @call
(arguments (identifier) @ref)
[(comment) (block_comment)] @comment
[(string) (interpolated_string_expression)] @string
