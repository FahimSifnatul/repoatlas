(function_definition declarator: (function_declarator declarator: [(identifier) @name (field_identifier) @name])) @func
(function_definition declarator: (function_declarator declarator: (qualified_identifier scope: (namespace_identifier) @owner name: (identifier) @name))) @func
(function_definition declarator: (pointer_declarator declarator: (function_declarator declarator: (identifier) @name))) @func
(function_definition declarator: (reference_declarator (function_declarator declarator: (identifier) @name))) @func
(class_specifier name: (type_identifier) @name body: (_)) @class
(struct_specifier name: (type_identifier) @name body: (_)) @class
(call_expression function: [(identifier) @callee (field_expression field: (field_identifier) @callee) (qualified_identifier name: (identifier) @callee) (template_function name: (identifier) @callee)]) @call
(new_expression type: (type_identifier) @callee) @call
(argument_list (identifier) @ref)
(comment) @comment
[(string_literal) (raw_string_literal)] @string
