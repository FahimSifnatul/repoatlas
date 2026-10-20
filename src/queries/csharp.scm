(class_declaration name: (identifier) @name) @class
(struct_declaration name: (identifier) @name) @class
(record_declaration name: (identifier) @name) @class
(enum_declaration name: (identifier) @name) @class
(interface_declaration name: (identifier) @name) @type
(method_declaration name: (identifier) @name) @func
(constructor_declaration name: (identifier) @name) @func
(local_function_statement name: (identifier) @name) @func
(invocation_expression function: [(identifier) @callee (member_access_expression name: (identifier) @callee) (generic_name (identifier) @callee)]) @call
(object_creation_expression type: [(identifier) @callee (generic_name (identifier) @callee) (qualified_name name: (identifier) @callee)]) @call
(argument (identifier) @ref)
(comment) @comment
[(string_literal) (verbatim_string_literal) (interpolated_string_expression) (raw_string_literal)] @string
