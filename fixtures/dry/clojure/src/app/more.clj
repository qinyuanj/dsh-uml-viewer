(ns app.more)

(def make-sum
  (fn [xs]
    (let [ys (filter odd? xs)]
      (map inc ys))))
