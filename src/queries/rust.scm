(function_item name: (identifier) @name) @func
(struct_item name: (type_identifier) @name) @class
(enum_item name: (type_identifier) @name) @class
(trait_item name: (type_identifier) @name) @class
(type_item name: (type_identifier) @name) @type
; methods in `impl Type` belong to Type
(impl_item type: [(type_identifier) @name (generic_type type: (type_identifier) @name) (scoped_type_identifier name: (type_identifier) @name)]) @impl
(call_expression function: [(identifier) @callee (field_expression field: (field_identifier) @callee) (scoped_identifier name: (identifier) @callee)
  (generic_function function: [(identifier) @callee (scoped_identifier name: (identifier) @callee) (field_expression field: (field_identifier) @callee)])]) @call
(arguments (identifier) @ref)
[(line_comment) (block_comment)] @comment
[(string_literal) (raw_string_literal)] @string
