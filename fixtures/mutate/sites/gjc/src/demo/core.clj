(ns demo.core)
(defn place [x]
  (if (> x 0)
    (+ x 1)
    0))
(defn- hide []
  false)
