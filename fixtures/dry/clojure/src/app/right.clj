(ns app.right)

(defn gamma [xs]
  (let [ys (filter odd? xs)]
    (map inc ys)
    (reduce + ys)))

(defn delta [items]
  (when (:ready items)
    {:count (count items)
     :first (first items)}))
