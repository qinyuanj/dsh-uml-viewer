(ns app.conditional)

(defn capped [n]
  (let [value (min n 100)
        bound (max value 0)]
    (when (> bound 1)
      bound)))

(defn declared [xs]
  (let [rows (set xs)
        labels (map name rows)]
    (into {} (map vector labels rows))))
