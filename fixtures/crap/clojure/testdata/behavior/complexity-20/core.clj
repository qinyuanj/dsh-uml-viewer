(defn foo [x]
  (loop [i 0] (recur (inc i))))