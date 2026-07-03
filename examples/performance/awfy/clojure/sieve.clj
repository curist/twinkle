;; Sieve over a Clojure persistent vector (32-way trie, functional `assoc`) —
;; the direct analog of Twinkle's persistent Vector<Bool>.
(defn sieve-mark [flags size i]
  (loop [k (* 2 i) f flags]
    (if (> k size) f (recur (+ k i) (assoc f k false)))))

(defn sieve-run [size]
  (loop [i 2 flags (vec (repeat (inc size) true)) count 0]
    (cond
      (> i size) count
      (nth flags i) (recur (inc i) (sieve-mark flags size i) (inc count))
      :else (recur (inc i) flags count))))

(def sieve-bench
  {:name "sieve" :warmup 10 :iters 40 :size 5000 :expected 669 :run sieve-run})

;; Unlocked tier: native Java primitive boolean-array, mutated in place —
;; Clojure's escape hatch to the native league.
(defn sieve-mut-run [size]
  (let [^booleans flags (boolean-array (inc size) true)]
    (loop [i 2 count 0]
      (cond
        (> i size) count
        (aget flags i)
        (do
          (loop [k (* 2 i)]
            (when (<= k size) (aset-boolean flags k false) (recur (+ k i))))
          (recur (inc i) (inc count)))
        :else (recur (inc i) count)))))

(def sieve-mut-bench
  {:name "sieve_mut" :warmup 10 :iters 40 :size 5000 :expected 669 :run sieve-mut-run})
