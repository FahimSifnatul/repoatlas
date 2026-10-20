; definitions
(class_declaration name: (_) @name) @class
(class name: (_) @name) @class
(function_declaration name: (identifier) @name) @func
(generator_function_declaration name: (identifier) @name) @func
(method_definition name: (property_identifier) @name) @func
(variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)]) @func
(assignment_expression left: (member_expression property: (property_identifier) @name) right: [(arrow_function) (function_expression)]) @func
(pair key: (property_identifier) @name value: [(arrow_function) (function_expression)]) @func
; calls, `new X()`, and functions handed to another call
(call_expression function: [(identifier) @callee (member_expression property: (property_identifier) @callee)]) @call
(new_expression constructor: [(identifier) @callee (member_expression property: (property_identifier) @callee)]) @call
(arguments (identifier) @ref)
; text that is not code
(comment) @comment
[(string) (template_string)] @string
(arguments (member_expression) @ref)
