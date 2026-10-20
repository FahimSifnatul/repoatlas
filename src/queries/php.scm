(class_declaration name: (name) @name) @class
(trait_declaration name: (name) @name) @class
(interface_declaration name: (name) @name) @type
(method_declaration name: (name) @name) @func
(function_definition name: (name) @name) @func
(function_call_expression function: [(name) @callee (qualified_name (name) @callee .)]) @call
(member_call_expression name: (name) @callee) @call
(nullsafe_member_call_expression name: (name) @callee) @call
(scoped_call_expression name: (name) @callee) @call
(object_creation_expression [(name) @callee (qualified_name (name) @callee .)]) @call
(comment) @comment
[(string) (encapsed_string)] @string
