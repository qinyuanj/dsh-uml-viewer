(ns app.broken)

(defn partial-copy [xs]
  (let [ys (filter odd? xs)]
    (map inc ys)))

(defn unfinished [x]
