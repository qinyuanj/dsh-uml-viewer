(ns app.fns)

(def make-total
  (fn [xs]
    (let [ys (filter odd? xs)]
      (map inc ys))))

(defn own [m]
  (update m :count inc))
