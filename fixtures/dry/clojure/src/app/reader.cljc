(ns app.reader)

(defn limit [n]
  (let [value (min n 100)
        bound (max value 0)]
    (when (> bound 1)
      bound)))

(defn ^String tagged [xs]
  #_(println xs)
  (let [rows (set xs)
        labels (map name rows)]
    (into {} (map vector labels rows))))

#?(:clj (defn conditional [n]
          (let [value (min n 100)
                bound (max value 0)]
            (when (> bound 1)
              bound)))
   :cljs (defn conditional [n]
           (let [value (max n 3)
                 bound (min value 0)]
             (when (< bound 1)
               bound))))
