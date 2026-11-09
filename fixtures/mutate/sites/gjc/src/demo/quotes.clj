(ns demo.quotes)
(defn place []
  (comment (+ 1 0))
  (quote (+ 1 0))
  '(+ 1 0)
  `(+ 1 0)
  ; (+ 1 0)
  (str "true + 1"))
