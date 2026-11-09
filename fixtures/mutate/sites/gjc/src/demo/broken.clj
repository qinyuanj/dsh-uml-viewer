(ns demo.broken)
(defn ok [x] (+ x 1))
(defn open [x]
  (if (> x 0) 1 0)
