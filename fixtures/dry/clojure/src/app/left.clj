(ns app.left)

(defn alpha [xs]
  (let [ys (filter odd? xs)]
    (map inc ys)
    (reduce + ys)))

(defn beta [items]
  (when (:ready items)
    {:count (count items)
     :first (first items)}))
