;; Clojure/JVM comparison for the dataframe order_by("amount", Asc) path.
;;
;; Run from repository root:
;;
;;   clojure examples/dataframe/bench/order_by_clojure.clj
;;
;; Mirrors examples/dataframe/bench/main.tw for order_by only:
;; generate key/amount/score columns, sort row indices by amount, gather all columns.

(set! *warn-on-reflection* true)

(import '[java.util Arrays Comparator])

(defrecord Table [^objects keys ^longs amounts ^doubles scores])

(defn next-seed ^long [^long seed]
  (mod (+ (* seed 1664525) 1013904223) 2147483648))

(defn gen-table ^Table [^long n ^long key-cardinality]
  (let [keys (object-array n)
        amounts (long-array n)
        scores (double-array n)]
    (loop [i 0
           seed 12345]
      (if (< i n)
        (let [seed1 (next-seed seed)
              k (mod seed1 key-cardinality)
              seed2 (next-seed seed1)
              amount (mod seed2 1000)
              seed3 (next-seed seed2)
              score (/ (double (mod seed3 10000)) 100.0)]
          (aset keys i (str "k" k))
          (aset-long amounts i amount)
          (aset-double scores i score)
          (recur (unchecked-inc-int i) seed3))
        (->Table keys amounts scores)))))

(defn gather ^Table [^Table t ^objects idx]
  (let [n (alength idx)
        in-keys ^objects (:keys t)
        in-amounts ^longs (:amounts t)
        in-scores ^doubles (:scores t)
        keys (object-array n)
        amounts (long-array n)
        scores (double-array n)]
    (dotimes [i n]
      (let [row (.intValue ^Number (aget idx i))]
        (aset keys i (aget in-keys row))
        (aset-long amounts i (aget in-amounts row))
        (aset-double scores i (aget in-scores row))))
    (->Table keys amounts scores)))

(defn order-by-amount-asc [^Table t]
  (let [amounts ^longs (:amounts t)
        n (alength amounts)
        idx (object-array n)]
    (dotimes [i n]
      (aset idx i (Long/valueOf i)))
    (let [sort-start (System/nanoTime)]
      (Arrays/sort
        idx
        (reify Comparator
          (compare [_ a b]
            (Long/compare
              (aget amounts (.intValue ^Number a))
              (aget amounts (.intValue ^Number b))))))
      (let [sort-elapsed (- (System/nanoTime) sort-start)
            gather-start (System/nanoTime)
            out (gather t idx)
            gather-elapsed (- (System/nanoTime) gather-start)]
        [out sort-elapsed gather-elapsed]))))

(defn ms [^long nanos]
  (/ (double nanos) 1000000.0))

(defn bench-n [^long n]
  (let [base (gen-table n 64)
        start (System/nanoTime)
        [sorted sort-elapsed gather-elapsed] (order-by-amount-asc base)
        total (- (System/nanoTime) start)
        checksum (alength ^longs (:amounts ^Table sorted))]
    (printf "N=%-8d clj total: %8.2fms  sort: %8.2fms  gather: %8.2fms  checksum %d%n"
            n (ms total) (ms sort-elapsed) (ms gather-elapsed) checksum)))

(doseq [n [10000 100000 1000000]]
  (bench-n n))
