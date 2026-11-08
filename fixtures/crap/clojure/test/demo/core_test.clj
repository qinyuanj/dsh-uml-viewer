(ns demo.core-test
  (:require [clojure.test :refer [deftest is run-tests]] [demo.core :as core]))
(deftest chooses-true (is (= 1 (core/choose true))))
(deftest counts-char (is (= \x (core/after-characters))))
(defn -main [& _] (let [r (run-tests 'demo.core-test)] (System/exit (+ (:fail r) (:error r)))))
