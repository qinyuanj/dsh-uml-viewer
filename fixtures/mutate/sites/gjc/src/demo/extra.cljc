(ns demo.extra)
(defn f [x] #?(:clj (inc x) :cljs (dec x)))
(defn g [x] (when-not (nil? x) (first x)))
