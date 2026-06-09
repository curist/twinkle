;; Clojure persistent-map comparison for the Twinkle Dict probe.
;;
;;   clojure examples/sort-bench/dict_perf_clojure.clj
;;
;; Uses plain `assoc`/`get` in a loop (NO transients) to compare the same
;; persistent operation family as Twinkle's Dict, which has no transient path.

(set! *warn-on-reflection* true)

(defn next-seed ^long [^long seed]
  (mod (+ (* seed 1664525) 1013904223) 2147483648))

(defn ms [^long nanos] (/ (double nanos) 1000000.0))

(defn bench-int [^long n]
  ;; build: keys 0..n-1
  (let [t0 (System/nanoTime)
        d (loop [i 0 m {}]
            (if (< i n)
              (recur (inc i) (assoc m i (mod (* i 7) 1000)))
              m))
        t1 (System/nanoTime)]
    (printf "int build  (n sets)        : %8.2fms (checksum %d)%n" (ms (- t1 t0)) (count d))
    ;; random get over present keys
    (let [g0 (System/nanoTime)
          acc (loop [c 0 seed 12345 acc 0]
                (if (< c n)
                  (let [s (next-seed seed)]
                    (recur (inc c) s (+ acc (get d (mod s n) 0))))
                  acc))
          g1 (System/nanoTime)]
      (printf "int get    (n random hits) : %8.2fms (checksum %d)%n" (ms (- g1 g0)) (mod acc 100000)))
    ;; random update of existing keys
    (let [u0 (System/nanoTime)
          e (loop [c 0 seed 999 m d]
              (if (< c n)
                (let [s (next-seed seed)]
                  (recur (inc c) s (assoc m (mod s n) (mod s 1000))))
                m))
          u1 (System/nanoTime)]
      (printf "int update (n random sets) : %8.2fms (checksum %d)%n" (ms (- u1 u0)) (count e)))))

(defn bench-string [^long n]
  (let [t0 (System/nanoTime)
        d (loop [i 0 m {}]
            (if (< i n)
              (recur (inc i) (assoc m (str "k" i) (mod (* i 7) 1000)))
              m))
        t1 (System/nanoTime)]
    (printf "str build  (n sets)        : %8.2fms (checksum %d)%n" (ms (- t1 t0)) (count d))
    (let [g0 (System/nanoTime)
          acc (loop [c 0 seed 12345 acc 0]
                (if (< c n)
                  (let [s (next-seed seed)]
                    (recur (inc c) s (+ acc (get d (str "k" (mod s n)) 0))))
                  acc))
          g1 (System/nanoTime)]
      (printf "str get    (n random hits) : %8.2fms (checksum %d)%n" (ms (- g1 g0)) (mod acc 100000)))))

(defn bench-n [^long n]
  (printf "── N = %d ──%n" n)
  (bench-int n)
  (bench-string n))

(println "=== clojure persistent-map probe ===")
(bench-n 1000000)
