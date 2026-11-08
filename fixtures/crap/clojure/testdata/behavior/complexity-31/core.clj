(defn foo [x]
  (cond-> x
    (pos? x) inc
    (even? x) (* 2)))