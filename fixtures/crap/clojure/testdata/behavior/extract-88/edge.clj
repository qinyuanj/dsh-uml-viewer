(defn alpha [x]
  (if x 1 0))

(def rules
  [{:pred (fn [v]
            (if v (when (pos? v) true) false))}])

(defn omega [y]
  y)