(class_declaration name: (identifier) @name) @class
(enum_declaration name: (identifier) @name) @class
(record_declaration name: (identifier) @name) @class
(interface_declaration name: (identifier) @name) @type
(method_declaration name: (identifier) @name) @func
(constructor_declaration name: (identifier) @name) @func
(method_invocation name: (identifier) @callee) @call
(object_creation_expression type: [(type_identifier) @callee (generic_type (type_identifier) @callee)]) @call
(argument_list (identifier) @ref)
[(line_comment) (block_comment)] @comment
(string_literal) @string
