(ns demo.heads
  (:require [clojure.string :as s]))

(defn ^:private tagged [x]
  (if (> x 0) (+ x 1) 0))

(defn every-head [xs n]
  (- n 1) (* n 1) (inc n) (dec n) (>= n 0) (<= n 1) (< n 0) (= n 1) (not= n 0)
  (if-not n 0 1) (when n 1) (when-not n 0) (and n 1) (or n 0)
  (double n) (int n) (first xs) (second xs) (filter odd? xs) (remove odd? xs)
  (take 1 xs) (drop 0 xs) (rest xs) (next xs) (every? odd? xs) (some odd? xs)
  (min n 1) (max n 0) (pos? n) (neg? n) (even? n) (odd? n) (nil? n) (some? n))

(defn not-heads [xs]
  (map inc xs)
  (:k inc 1)
  (#?(:clj inc :cljs dec) 1)
  #?@(:clj [(+ 1 0)])
  ^{:tag int} (inc 0)
  (^:m inc 0)
  (clojure.core/inc 0)
  (foo/bar 1)
  #(+ % 1)
  (;c
   inc 1)
  (#_skip inc 1)
  #_(+ 1 0)
  ((+ 1 0))
  (. xs inc 1))

(defn literals []
  [nil 1.0 -1 +1 0x1 1N 01 1/2 0.0 \a \0 \newline "s\"x 1" #"re+1" ##Inf true false 1 0]
  #{1 0} {:a 1 0 true} #:a{:b 1} #inst "2020" @(deref 1) #'inc
  `(+ ~x ~@xs 1) ~(inc 1) #=(+ 1 0)
  (truex falsey true? false?)
  {:true true :false false}
  ^{:doc true} [true])

(defn unicode [x]
  ;; ünïcode – comments 1
  (str "ß ✓" x) (+ x 1) (λ 0) (é 1))

(defn reader [x]
  #?(:clj (+ x 1) :cljs (- x 1))
  #_#_ (+ 1 0) (- 1 0)
  (comment) (quote) (if)
  (-> x inc (- 1))
  (->> x (+ 1))
  (let [y (* x 1)] (when (pos? y) y)))

(defmacro m [x] `(if ~x 1 0))

(defn multi
  ([] (multi 0))
  ([x] (if x 1 0)))

(def not-a-fn (+ 1 0))

(defn crlf [x]
  (+ x 1))
