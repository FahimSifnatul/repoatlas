(class name: [(constant) @name (scope_resolution name: (constant) @name)]) @class
(module name: [(constant) @name (scope_resolution name: (constant) @name)]) @class
(method name: (_) @name) @func
(singleton_method name: (_) @name) @func
(call method: [(identifier) @callee (constant) @callee]) @call
; if __FILE__ == $0
(program (if) @guard)
; a method called with no arguments or parentheses: `authenticate_user!`
(body_statement (identifier) @callee @call)
(do_block (body_statement (identifier) @callee @call))
(comment) @comment
(string) @string
