(function_definition declarator: (function_declarator declarator: (identifier) @name)) @func
(function_definition declarator: (pointer_declarator declarator: (function_declarator declarator: (identifier) @name))) @func
(struct_specifier name: (type_identifier) @name body: (_)) @class
(enum_specifier name: (type_identifier) @name body: (_)) @class
(call_expression function: [(identifier) @callee (field_expression field: (field_identifier) @callee)]) @call
(argument_list (identifier) @ref)
(comment) @comment
(string_literal) @string
