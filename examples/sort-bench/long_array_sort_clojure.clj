;; Clojure/JVM sort calibration for primitive long[] vs value-style collections.
;;
;; Run from repository root:
;;
;;   clojure examples/sort-bench/long_array_sort_clojure.clj
;;
;; Clojure's ordinary `sort` preserves value-style usage by sorting an Object[]
;; copy and returning a seq; primitive `long[]` sorting is Java interop
;; (`java.util.Arrays/sort`) and is in-place. The "long-array-copy" case clones
;; before sorting to model value-preserving use while still measuring the dense
;; primitive kernel.

(set! *warn-on-reflection* true)

(import '[java.util Arrays])

(def default-n 1000000)
(def warmups 3)
(def runs 5)

(defn parse-long-or [s ^long fallback]
  (if s
    (Long/parseLong s)
    fallback))

(defn next-seed ^long [^long seed]
  (mod (+ (* seed 1664525) 1013904223) 2147483648))

(defn ms ^double [^long nanos]
  (/ (double nanos) 1000000.0))

(defn gen-long-array [^long n]
  (let [a (long-array n)]
    (loop [i 0 seed 12345]
      (when (< i n)
        (let [seed1 (next-seed seed)
              x (rem seed1 1000000)
              seed2 (next-seed seed1)]
          (aset-long a i x)
          (recur (unchecked-inc i) seed2))))
    a))

(defn gen-pvec [^longs a]
  (loop [i 0 v (transient [])]
    (if (< i (alength a))
      (recur (unchecked-inc i) (conj! v (aget a i)))
      (persistent! v))))

(defn checksum-array ^long [^longs a]
  (let [n (alength a)]
    (unchecked-add (aget a 0) (aget a (unchecked-dec n)))))

(defn checksum-seq ^long [xs]
  (unchecked-add (long (first xs)) (long (last xs))))

(defn sort-long-array-copy ^long [^longs a]
  (let [b (aclone a)]
    (Arrays/sort b)
    (checksum-array b)))

(defn sort-pvec-core ^long [v]
  (let [s (sort v)]
    (checksum-seq s)))

(defn sort-pvec-vec ^long [v]
  (let [s (vec (sort v))]
    (checksum-seq s)))

(defn timed [label f x]
  (dotimes [_ warmups]
    (f x))
  (dotimes [r runs]
    (let [t0 (System/nanoTime)
          checksum (f x)
          elapsed (- (System/nanoTime) t0)]
      (printf "%s run%d %8.2fms checksum=%d%n" label r (ms elapsed) checksum))))

(defn -main [& args]
  (let [n (parse-long-or (first args) default-n)]
    (printf "n=%d warmups=%d runs=%d%n" n warmups runs)
    (println "building inputs...")
    (let [a (gen-long-array n)
          v (gen-pvec a)]
      (println "sorting...")
      (timed "long-array-copy Arrays/sort" sort-long-array-copy a)
      (timed "pvec core/sort seq       " sort-pvec-core v)
      (timed "pvec vec(sort)          " sort-pvec-vec v))))

(apply -main *command-line-args*)
