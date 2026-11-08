(ns app.extra-test)

(defn alpha [xs]
  (let [ys (filter odd? xs)]
    (map inc ys)
    (reduce + ys)))
