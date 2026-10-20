(function_declaration name: (identifier) @name) @func
(method_declaration
  receiver: (parameter_list (parameter_declaration type: [(type_identifier) @owner (pointer_type (type_identifier) @owner) (generic_type type: (type_identifier) @owner) (pointer_type (generic_type type: (type_identifier) @owner))]))
  name: (field_identifier) @name) @func
(type_spec name: (type_identifier) @name type: (struct_type)) @class
(type_spec name: (type_identifier) @name type: (interface_type)) @type
(call_expression function: [(identifier) @callee (selector_expression field: (field_identifier) @callee)]) @call
(argument_list (identifier) @ref)
(comment) @comment
[(interpreted_string_literal) (raw_string_literal)] @string
(argument_list (selector_expression) @ref)
