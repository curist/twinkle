;; Clojure persistent-vector read probe for reference payloads.
;;
;; Run from repository root:
;;
;;   clojure examples/sort-bench/ref_vector_read_clojure.clj
;;
;; This is meant to calibrate Twinkle's typed-vector direction against Clojure's
;; ordinary persistent vectors. It compares the same random-read shape across:
;;
;;   * primitive long-array     — non-boxed dense JVM baseline
;;   * boxed Long values        — primitive payload boxed in a reference vector
;;   * String references        — reference payload, no scalar unboxing
;;   * deftype Row references   — record-like object payload with field reads
;;   * map rows                 — hash-map record-like payload, intentionally slower
;;
;; The important question is not absolute Clojure-vs-Twinkle speed; it is whether
;; reference payload vectors have a fundamentally different penalty profile than
;; boxed primitive payload vectors.

(set! *warn-on-reflection* true)

(def default-n 1000000)
(def default-m 10000000)
(def warmups 3)
(def runs 5)

(deftype Row [^long id ^String name])

(defn parse-long-or [s ^long fallback]
  (if s
    (Long/parseLong s)
    fallback))

(defn ms ^double [^long nanos]
  (/ (double nanos) 1000000.0))

(defn idx ^long [^long k ^long n]
  ;; Same multiplicative random-ish access pattern used by the Twinkle probe,
  ;; masked positive so rem stays non-negative under JVM signed overflow.
  (rem (bit-and (unchecked-multiply k 2654435761) 0x7fffffffffffffff) n))

(defn gen-long-array [^long n]
  (let [a (long-array n)]
    (dotimes [i n]
      (aset-long a i i))
    a))

(defn gen-longs [^long n]
  (loop [i 0 v (transient [])]
    (if (< i n)
      (recur (unchecked-inc i) (conj! v i))
      (persistent! v))))

(defn gen-strings [^long n]
  (loop [i 0 v (transient [])]
    (if (< i n)
      (recur (unchecked-inc i) (conj! v (str "s" i)))
      (persistent! v))))

(defn gen-rows [^long n]
  (loop [i 0 v (transient [])]
    (if (< i n)
      (recur (unchecked-inc i) (conj! v (Row. i (str "s" i))))
      (persistent! v))))

(defn gen-map-rows [^long n]
  (loop [i 0 v (transient [])]
    (if (< i n)
      (recur (unchecked-inc i) (conj! v {:id i :name (str "s" i)}))
      (persistent! v))))

(defn sum-long-array ^long [^longs a ^long m ^long n]
  (loop [k 0 acc 0]
    (if (< k m)
      (let [x (aget a (idx k n))]
        (recur (unchecked-inc k) (unchecked-add acc x)))
      acc)))

(defn sum-longs ^long [v ^long m ^long n]
  (loop [k 0 acc 0]
    (if (< k m)
      (let [x ^long (nth v (idx k n))]
        (recur (unchecked-inc k) (unchecked-add acc x)))
      acc)))

(defn sum-string-lens ^long [v ^long m ^long n]
  (loop [k 0 acc 0]
    (if (< k m)
      (let [s ^String (nth v (idx k n))]
        (recur (unchecked-inc k) (unchecked-add acc (.length s))))
      acc)))

(defn sum-row-fields ^long [v ^long m ^long n]
  (loop [k 0 acc 0]
    (if (< k m)
      (let [r ^Row (nth v (idx k n))
            name ^String (.-name r)]
        (recur (unchecked-inc k) (unchecked-add acc (long (unchecked-add (.-id r) (.length name))))))
      acc)))

(defn sum-map-row-fields ^long [v ^long m ^long n]
  (loop [k 0 acc 0]
    (if (< k m)
      (let [r (nth v (idx k n))
            id (long (:id r))
            name ^String (:name r)]
        (recur (unchecked-inc k) (unchecked-add acc (long (unchecked-add id (.length name))))))
      acc)))

(defn timed [label f v m n]
  (dotimes [_ warmups]
    (f v m n))
  (dotimes [r runs]
    (let [t0 (System/nanoTime)
          checksum (f v m n)
          elapsed (- (System/nanoTime) t0)]
      (printf "%s run%d %8.2fms checksum=%d%n" label r (ms elapsed) checksum))))

(defn -main [& args]
  (let [n (parse-long-or (first args) default-n)
        m (parse-long-or (second args) default-m)]
    (printf "n=%d m=%d warmups=%d runs=%d%n" n m warmups runs)
    (println "building vectors...")
    (let [long-array (gen-long-array n)
          longs (gen-longs n)
          strings (gen-strings n)
          rows (gen-rows n)
          map-rows (gen-map-rows n)]
      (println "benching random reads...")
      (timed "long-array" sum-long-array long-array m n)
      (timed "boxed-long" sum-longs longs m n)
      (timed "string-ref" sum-string-lens strings m n)
      (timed "row-ref   " sum-row-fields rows m n)
      (timed "map-row   " sum-map-row-fields map-rows m n))))

(apply -main *command-line-args*)
