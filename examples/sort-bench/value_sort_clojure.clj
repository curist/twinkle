;; Clojure persistent-vector reference for a plain value sort.
;;
;; Run from repository root:
;;
;;   clojure examples/sort-bench/value_sort_clojure.clj
;;
;; This intentionally uses Clojure's ordinary persistent vectors (not Java
;; primitive arrays) to compare the same broad representation family as
;; Twinkle's public Vector<T>. The timed call is `(vec (sort v))`, warmed.

(set! *warn-on-reflection* true)

(defn next-seed ^long [^long seed]
  (mod (+ (* seed 1664525) 1013904223) 2147483648))

;; Build a persistent vector of n LCG-derived ints (same shape as the Twinkle
;; bench: lots of duplicates, range 0..999999).
(defn gen-vec [^long n]
  (loop [i 0
         seed 12345
         v (transient [])]
    (if (< i n)
      (let [seed1 (next-seed seed)
            x (mod seed1 1000000)
            seed2 (next-seed seed1)]
        (recur (unchecked-inc-int i) seed2 (conj! v x)))
      (persistent! v))))

(defn ms [^long nanos]
  (/ (double nanos) 1000000.0))

(defn timed-sort [v]
  (let [start (System/nanoTime)
        sorted (vec (sort v))
        elapsed (- (System/nanoTime) start)]
    [sorted elapsed]))

(defn bench-n [^long n warmups]
  (let [v (gen-vec n)]
    ;; Warm up the JIT.
    (dotimes [_ warmups]
      (timed-sort v))
    (let [[sorted elapsed] (timed-sort v)]
      (printf "N=%-8d clj-pvec sort: %8.2fms  len %d first %d last %d%n"
              n (ms elapsed) (count sorted) (first sorted) (last sorted)))))

(bench-n 10000 50)
(bench-n 100000 20)
(bench-n 1000000 10)
