;; Clojure persistent-vector comparison for the dataframe order_by("amount", Asc) path.
;;
;; Run from repository root:
;;
;;   clojure examples/dataframe/bench/order_by_clojure_persistent.clj
;;
;; This intentionally uses Clojure's ordinary persistent vectors instead of Java
;; primitive arrays, to compare the same broad representation family as Twinkle's
;; public Vector<T>: indexed persistent collections plus an idiomatic sort over
;; row indices.

(set! *warn-on-reflection* true)

(defrecord Table [keys amounts scores])

(defn next-seed ^long [^long seed]
  (mod (+ (* seed 1664525) 1013904223) 2147483648))

(defn gen-table ^Table [^long n ^long key-cardinality]
  (loop [i 0
         seed 12345
         keys []
         amounts []
         scores []]
    (if (< i n)
      (let [seed1 (next-seed seed)
            k (mod seed1 key-cardinality)
            seed2 (next-seed seed1)
            amount (mod seed2 1000)
            seed3 (next-seed seed2)
            score (/ (double (mod seed3 10000)) 100.0)]
        (recur (unchecked-inc-int i)
               seed3
               (conj keys (str "k" k))
               (conj amounts amount)
               (conj scores score)))
      (->Table keys amounts scores))))

(defn gather ^Table [^Table t idx]
  (let [in-keys (:keys t)
        in-amounts (:amounts t)
        in-scores (:scores t)]
    (->Table
      (mapv #(nth in-keys %) idx)
      (mapv #(nth in-amounts %) idx)
      (mapv #(nth in-scores %) idx))))

(defn order-by-amount-asc [^Table t]
  (let [amounts (:amounts t)
        n (count amounts)
        idx (vec (range n))]
    (let [sort-start (System/nanoTime)
          ;; Keep the key-index idiom: sort row ids by looking up their key in a
          ;; persistent vector. `sort-by` returns a seq, so realize it as a vector
          ;; before gather to keep the phases explicit.
          sorted (vec (sort-by #(nth amounts %) idx))
          sort-elapsed (- (System/nanoTime) sort-start)
          gather-start (System/nanoTime)
          out (gather t sorted)
          gather-elapsed (- (System/nanoTime) gather-start)]
      [out sort-elapsed gather-elapsed])))

(defn ms [^long nanos]
  (/ (double nanos) 1000000.0))

(defn bench-n [^long n]
  (let [base (gen-table n 64)
        start (System/nanoTime)
        [sorted sort-elapsed gather-elapsed] (order-by-amount-asc base)
        total (- (System/nanoTime) start)
        checksum (count (:amounts ^Table sorted))]
    (printf "N=%-8d clj-pvec total: %8.2fms  sort: %8.2fms  gather: %8.2fms  checksum %d%n"
            n (ms total) (ms sort-elapsed) (ms gather-elapsed) checksum)))

(doseq [n [10000 100000 1000000]]
  (bench-n n))
