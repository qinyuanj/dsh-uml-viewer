(ns mix)
(defn mix [a b] (bit-xor (bit-and a b) (bit-or a b)))
