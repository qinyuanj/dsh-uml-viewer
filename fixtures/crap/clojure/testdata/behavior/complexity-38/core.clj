(defn foo [x]
  (some->> x (map inc) (filter pos?)))