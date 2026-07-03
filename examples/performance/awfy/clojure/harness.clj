;; Timing + checksum harness. Loaded via load-file, so defs land in the
;; default namespace alongside the per-benchmark files.
;;
;; Unchecked math (applies to the files loaded after this one) so the native
;; array ports aren't paying overflow checks on every op.
(set! *unchecked-math* true)
(defn run-bench [{:keys [name warmup iters size expected run]}]
  (dotimes [_ warmup] (run size))
  (let [start (System/nanoTime)
        checksum (loop [i 0 c 0] (if (< i iters) (recur (inc i) (run size)) c))
        elapsed (/ (double (- (System/nanoTime) start)) 1e6)]
    (when (not= checksum expected)
      (throw (ex-info (str name ": checksum " checksum " != expected " expected) {})))
    (println (str "clojure\t" name "\t" iters "\t" elapsed "\t" checksum))))

(defn run-all [benches]
  (doseq [b benches] (run-bench b)))
