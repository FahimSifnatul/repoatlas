; definitions
(class_definition name: (identifier) @name) @class
(function_definition name: (identifier) @name) @func
; calls, and functions handed to another call: Thread(target=run), map(fn, xs)
(call function: [(identifier) @callee (attribute attribute: (identifier) @callee)]) @call
(argument_list (identifier) @ref)
(keyword_argument value: (identifier) @ref)
; if __name__ == "__main__":
(module (if_statement) @guard)
; text that is not code
(comment) @comment
(expression_statement (string) @doc)
(string) @string
; an object built at the top of another module, handed over: include_router(items.router)
(argument_list (attribute) @ref)
(keyword_argument value: (attribute) @ref)
