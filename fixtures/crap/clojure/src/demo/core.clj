(ns demo.core)

(defn choose [x]
  (if x 1 0))

(defn- hidden [y] y)

(defn branches [x]
  (if-not x 0 1)
  (if-let [y x] y 0)
  (if-some [y x] y 0)
  (when x 1)
  (when-not x 1)
  (when-let [y x] y)
  (when-some [y x] y)
  (when-first [y [x]] y)
  (and x true)
  (or x false)
  (loop [i 0] i)
  (try x (catch Exception e nil)))

(defn clauses [x]
  (cond (= x 1) {:a 1 :b 2} :else nil)
  (condp = x 1 :one 2 :two)
  (case x () :empty [1] :one :other)
  (cond-> x (number? x) inc (number? x) dec)
  (cond->> x (number? x) inc)
  (some-> x inc dec)
  (some->> [x] (map inc) (filter pos?)))

(defn strings [x]
  "say \"hi\"\n (if true 1 0)"
  "" "\\" "\""
  ;; (when fake)
  \space
  (if x 1 0))

(def data [{:pred (fn [v] (if v (when v true) false))}])
(def char-literals [\a \( \" \space])
(defn after-characters [] \x)
(defn fallback-clause [x] (cond x 1 :else nil))
(defn empty-clauses [x] (cond) (some-> x))
