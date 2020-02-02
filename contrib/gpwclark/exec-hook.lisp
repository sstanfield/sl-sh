;; This is a sl-sh file for people named price, you would put it in ~/.exec-hook.lisp to use it.
(core::ns-import 'core)
(ns-import 'shell)

;; exec hook fcn {{{

;; fcnality
;; 1. entering 1 arg on the CLI that is a valid directory results in changing
;;		to that directory.
;; 2. support $la env variable which is set to the last argument of the previous
;;		command input.
;; TODOS
;; 1. support things like $2la for second to last arg, $3la, etc..
;; 2. organize fcns
;; 3. how to handle mixing infix notation, e.g.
;;		cat file | grep "stuff" out> afile
;; 3. use docstrings


	(defn change-dir-if-arg-is-dir (cmd)
		(let ((cmd-str (str cmd)))
			(if (fs-dir? cmd-str)
				(list root::cd cmd-str)
				cmd-str)))

	(defn apply-ast-order (ast order)
		(if (< (length ast) 3)
			(err "Expressions with infix symbols must have at least 3 forms")
			(match order
				(:as-is ast)
				(:swap-last-for-first (append (list (first ast)) (last ast) (butlast (rest ast))))
				(nil (err "Unable to apply ordering, unknown order symbol.")))))

	(defn prefixify-cmd (cmd-toks prefix-props)
		(progn
			(defq apply-xform (fn (x) (if (not (= (hash-get prefix-props :infix-symbol) (last x))) (vec-push! x (((hash-get prefix-props :xform)) (vec-pop! x))))))
			(defq build-cmd (fn (cmd-ast raw-list)
				(progn
					(defq next-tok (first raw-list))
					(defq infix-symbol (hash-get prefix-props :infix-symbol))
					(if (not next-tok)
						(apply-xform cmd-ast)
						(progn
							(recur
								(if (= infix-symbol next-tok)
									(progn
										(apply-xform cmd-ast)
										  (append!
											cmd-ast
											(vec (make-vec)))
											cmd-ast)
									(progn
										(append!
											(last cmd-ast)
											(if (seq? next-tok) next-tok (vec next-tok)))
										cmd-ast))
								(rest raw-list)))))))
			(defq prefixified-ast
				(build-cmd (vec (hash-get prefix-props :prefix-symbol) (make-vec))
				cmd-toks))
			(setq prefixified-ast (apply-ast-order prefixified-ast (hash-get prefix-props :ast-order)))
			prefixified-ast))

	;; return true if cmd satisfies preconditions for prefixification
	(defn satisfies-prefixify-preconditions (cmd-ast infix-hash-set)
		(let ((infix-symbol (hash-get infix-hash-set :infix-symbol)))
			(not (or ;; reasons to skip pre-processing
				;; if read returns nil
				(not cmd-ast)
				;; if cmd doesn't contain symbol
				(not (in? cmd-ast infix-symbol))
				;; if already in prefix notation
				(= infix-symbol (first cmd-ast))))))

	(defn confirm-prefix-eligible (cmd-ast prefix-metadata)
		;; recurse over prefix-metadata return nil if there are none but
		;; return the pair if it's valid
		(progn (defq prefix-props (first prefix-metadata))
			(if (not prefix-props)
				nil
				(if (satisfies-prefixify-preconditions cmd-ast prefix-props)
					prefix-props
					(recur cmd-ast (rest prefix-metadata))))))

	(defn identity ()
		(fn (x) x))

	(defn handle-process (cmd-proc)
		(if (process? cmd-proc) (= 0 (wait cmd-proc)) (not (not cmd-proc))))

	(defmacro proc-wait ()
		(fn (cmd) `(handle-process ,cmd)))

	(defn gen-prefix-data (infix-symbol prefix-symbol xform ast-order)
		(progn
			(defq prefix-props (make-hash))
			(hash-set! prefix-props :infix-symbol infix-symbol)
			(hash-set! prefix-props :prefix-symbol prefix-symbol)
			(hash-set! prefix-props :xform xform)
			(hash-set! prefix-props :ast-order ast-order)
			prefix-props))

	;; TODO this function could/should be applied recursively to handle any
	;; prefixification needs in inner forms.
	(defn check-for-infix-notation (cmd-ast)
		;; confirm cmd ast needs prefixification ...then call prefixify.
		(progn
				(defq prefix-metadata
					(list
						(gen-prefix-data '|| 'or proc-wait :as-is #| this '|| form must come before the '| form|#)
						(gen-prefix-data '| '| identity :as-is #| this '| form must come after the '|| form|#)
						(gen-prefix-data '@@ 'progn identity :as-is)
						(gen-prefix-data '&& 'and proc-wait :as-is)
						(gen-prefix-data 'out> 'out> identity :swap-last-for-first)
						(gen-prefix-data 'out>> 'out>> identity :swap-last-for-first)
						(gen-prefix-data 'err> 'err> identity :swap-last-for-first)
						(gen-prefix-data 'err>> 'err>> identity :swap-last-for-first)
						(gen-prefix-data 'out>null 'out>null identity :swap-last-for-first)
						(gen-prefix-data 'out-err> 'out-err> identity :swap-last-for-first)
						(gen-prefix-data 'out-err>> 'out-err>> identity :swap-last-for-first)
						(gen-prefix-data 'out-err>null 'out-err>null identity :swap-last-for-first)))
				(defq prefix-eligible
					(confirm-prefix-eligible cmd-ast prefix-metadata))
				;;TODO could loop by the infix symbols. this would allow us to
				;; detect forms with infix symbols nested within infix symbols.
				;; would start by finding the first instance of an infix symbol
				;; in the ast. then group all forms after in list, until
				;; occurrence of next of that infix symbol. then recursive
				;; aplication could properly prefixify everything.
			(if (not prefix-eligible)
				cmd-ast
				(prefixify-cmd cmd-ast prefix-eligible))))

	(defn recursively-check-for-infix-notation (new-ast orig-ast)
		(progn
			(defq fst (first orig-ast))
			(defq rst (rest orig-ast))
			(if (not fst)
				(check-for-infix-notation new-ast)
				(recur (append new-ast
							(if (non-empty-seq? fst)
								(progn
									(defq prefixified-subform (check-for-infix-notation fst))
									(vec (recursively-check-for-infix-notation (make-vec) prefixified-subform)))
								(vec fst)))
					rst))))

	(defn __exec_hook (cmd-str)
		(let ((cmd-ast (read :add-parens cmd-str)))
				(match (length cmd-ast)
					(1 (change-dir-if-arg-is-dir (first cmd-ast)))
					(nil (recursively-check-for-infix-notation (make-vec) cmd-ast)))))
;; }}}
