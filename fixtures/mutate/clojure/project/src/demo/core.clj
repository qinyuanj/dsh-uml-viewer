(ns demo.core
  (:require [clojure.string :as str]))

(defn clamp [x lo hi]
  (cond
    (< x lo) lo
    (> x hi) hi
    :else x))

(defn total [xs]
  (reduce + 0 xs))

(defn- shout [s]
  (when (seq s)
    (str/upper-case s)))

(defn loud [s]
  (shout s))

(defn evens [xs]
  (filter even? xs))

(defn ends [xs]
  [(first xs) (last xs)])

(defn ready? [flag]
  (and flag true))

(defn untested [n]
  (inc n))

(defn quoted []
  ;; (+ 1 0) in a comment and '(+ 1 0) quoted are never mutated.
  '(+ 1 0))
