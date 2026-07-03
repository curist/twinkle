;; NBody over a Clojure persistent vector of immutable body vectors. Each
;; velocity/position update rebinds the outer vector with `assoc`. Constants and
;; the arithmetic order mirror the other ports exactly so the scaled-integer
;; energy checksum agrees bit-for-bit (JVM doubles, no FMA fusion).
;;
;; Body layout: [x y z vx vy vz mass].
(def ^:const nb-pi 3.141592653589793)
(def ^:const nb-solar-mass (* 4.0 nb-pi nb-pi))
(def ^:const nb-days-per-year 365.24)

(defn nb-init []
  (let [bodies
        [[0.0 0.0 0.0 0.0 0.0 0.0 nb-solar-mass] ; Sun
         [4.84143144246472090 -1.16032004402742839 -1.03622044471123109e-01
          (* 1.66007664274403694e-03 nb-days-per-year)
          (* 7.69901118419740425e-03 nb-days-per-year)
          (* -6.90460016972063023e-05 nb-days-per-year)
          (* 9.54791938424326609e-04 nb-solar-mass)] ; Jupiter
         [8.34336671824457987 4.12479856412430479 -4.03523417114321381e-01
          (* -2.76742510726862411e-03 nb-days-per-year)
          (* 4.99852801234917238e-03 nb-days-per-year)
          (* 2.30417297573763929e-05 nb-days-per-year)
          (* 2.85885980666130812e-04 nb-solar-mass)] ; Saturn
         [1.28943695621391310e+01 -1.51111514016986312e+01 -2.23307578892655734e-01
          (* 2.96460137564761618e-03 nb-days-per-year)
          (* 2.37847173959480950e-03 nb-days-per-year)
          (* -2.96589568540237556e-05 nb-days-per-year)
          (* 4.36624404335156298e-05 nb-solar-mass)] ; Uranus
         [1.53796971148509165e+01 -2.59193146099879641e+01 1.79258772950371181e-01
          (* 2.68067772490389322e-03 nb-days-per-year)
          (* 1.62824170038242295e-03 nb-days-per-year)
          (* -9.51592254519715870e-05 nb-days-per-year)
          (* 5.15138902046611451e-05 nb-solar-mass)]] ; Neptune
        px (reduce (fn [a b] (+ a (* (nth b 3) (nth b 6)))) 0.0 bodies)
        py (reduce (fn [a b] (+ a (* (nth b 4) (nth b 6)))) 0.0 bodies)
        pz (reduce (fn [a b] (+ a (* (nth b 5) (nth b 6)))) 0.0 bodies)
        s (nth bodies 0)]
    (assoc bodies 0
           [(nth s 0) (nth s 1) (nth s 2)
            (/ (- px) nb-solar-mass) (/ (- py) nb-solar-mass) (/ (- pz) nb-solar-mass)
            (nth s 6)])))

(defn nb-advance [bodies dt]
  (let [n (count bodies)
        bs (loop [i 0 bs bodies]
             (if (< i n)
               (let [bi (nth bs i)
                     bix (nth bi 0) biy (nth bi 1) biz (nth bi 2) bimass (nth bi 6)
                     [bs2 vx vy vz]
                     (loop [j (inc i) bs bs vx (nth bi 3) vy (nth bi 4) vz (nth bi 5)]
                       (if (< j n)
                         (let [bj (nth bs j)
                               dx (- bix (nth bj 0))
                               dy (- biy (nth bj 1))
                               dz (- biz (nth bj 2))
                               d2 (+ (* dx dx) (* dy dy) (* dz dz))
                               mag (/ dt (* d2 (Math/sqrt d2)))
                               bjmass (nth bj 6)
                               bj2 [(nth bj 0) (nth bj 1) (nth bj 2)
                                    (+ (nth bj 3) (* dx bimass mag))
                                    (+ (nth bj 4) (* dy bimass mag))
                                    (+ (nth bj 5) (* dz bimass mag))
                                    bjmass]]
                           (recur (inc j) (assoc bs j bj2)
                                  (- vx (* dx bjmass mag))
                                  (- vy (* dy bjmass mag))
                                  (- vz (* dz bjmass mag))))
                         [bs vx vy vz]))]
                 (recur (inc i) (assoc bs2 i [bix biy biz vx vy vz bimass])))
               bs))]
    (loop [k 0 bs bs]
      (if (< k n)
        (let [b (nth bs k)]
          (recur (inc k)
                 (assoc bs k [(+ (nth b 0) (* dt (nth b 3)))
                              (+ (nth b 1) (* dt (nth b 4)))
                              (+ (nth b 2) (* dt (nth b 5)))
                              (nth b 3) (nth b 4) (nth b 5) (nth b 6)])))
        bs))))

(defn nb-energy [bodies]
  (let [n (count bodies)]
    (loop [i 0 e 0.0]
      (if (< i n)
        (let [bi (nth bodies i)
              e1 (+ e (* 0.5 (nth bi 6)
                         (+ (* (nth bi 3) (nth bi 3))
                            (* (nth bi 4) (nth bi 4))
                            (* (nth bi 5) (nth bi 5)))))
              e2 (loop [j (inc i) e e1]
                   (if (< j n)
                     (let [bj (nth bodies j)
                           dx (- (nth bi 0) (nth bj 0))
                           dy (- (nth bi 1) (nth bj 1))
                           dz (- (nth bi 2) (nth bj 2))
                           dist (Math/sqrt (+ (* dx dx) (* dy dy) (* dz dz)))]
                       (recur (inc j) (- e (/ (* (nth bi 6) (nth bj 6)) dist))))
                     e))]
          (recur (inc i) e2))
        e))))

(defn nbody-run [size]
  (let [final (loop [i 0 bodies (nb-init)]
                (if (< i size) (recur (inc i) (nb-advance bodies 0.01)) bodies))]
    (long (Math/round (* (nb-energy final) 1e8)))))

(def nbody-bench
  {:name "nbody" :warmup 5 :iters 20 :size 20000 :expected -16908926 :run nbody-run})

;; Unlocked tier: 5 bodies packed into a native double-array (7 fields each),
;; mutated in place — Clojure's escape hatch to the native league. Body b,
;; field f -> index (b*7 + f).
(defn nb-mut-init []
  (let [^doubles a (double-array 35)]
    (aset-double a 6 nb-solar-mass) ; Sun mass
    (let [set-body (fn [b x y z vx vy vz mass]
                     (aset-double a (+ (* b 7) 0) x) (aset-double a (+ (* b 7) 1) y)
                     (aset-double a (+ (* b 7) 2) z) (aset-double a (+ (* b 7) 3) vx)
                     (aset-double a (+ (* b 7) 4) vy) (aset-double a (+ (* b 7) 5) vz)
                     (aset-double a (+ (* b 7) 6) mass))]
      (set-body 1 4.84143144246472090 -1.16032004402742839 -1.03622044471123109e-01
                (* 1.66007664274403694e-03 nb-days-per-year) (* 7.69901118419740425e-03 nb-days-per-year)
                (* -6.90460016972063023e-05 nb-days-per-year) (* 9.54791938424326609e-04 nb-solar-mass))
      (set-body 2 8.34336671824457987 4.12479856412430479 -4.03523417114321381e-01
                (* -2.76742510726862411e-03 nb-days-per-year) (* 4.99852801234917238e-03 nb-days-per-year)
                (* 2.30417297573763929e-05 nb-days-per-year) (* 2.85885980666130812e-04 nb-solar-mass))
      (set-body 3 1.28943695621391310e+01 -1.51111514016986312e+01 -2.23307578892655734e-01
                (* 2.96460137564761618e-03 nb-days-per-year) (* 2.37847173959480950e-03 nb-days-per-year)
                (* -2.96589568540237556e-05 nb-days-per-year) (* 4.36624404335156298e-05 nb-solar-mass))
      (set-body 4 1.53796971148509165e+01 -2.59193146099879641e+01 1.79258772950371181e-01
                (* 2.68067772490389322e-03 nb-days-per-year) (* 1.62824170038242295e-03 nb-days-per-year)
                (* -9.51592254519715870e-05 nb-days-per-year) (* 5.15138902046611451e-05 nb-solar-mass)))
    (let [px (loop [b 0 acc 0.0] (if (< b 5) (recur (inc b) (+ acc (* (aget a (+ (* b 7) 3)) (aget a (+ (* b 7) 6))))) acc))
          py (loop [b 0 acc 0.0] (if (< b 5) (recur (inc b) (+ acc (* (aget a (+ (* b 7) 4)) (aget a (+ (* b 7) 6))))) acc))
          pz (loop [b 0 acc 0.0] (if (< b 5) (recur (inc b) (+ acc (* (aget a (+ (* b 7) 5)) (aget a (+ (* b 7) 6))))) acc))]
      (aset-double a 3 (/ (- px) nb-solar-mass))
      (aset-double a 4 (/ (- py) nb-solar-mass))
      (aset-double a 5 (/ (- pz) nb-solar-mass)))
    a))

(defn nb-mut-advance [^doubles a ^double dt]
  (dotimes [i 5]
    (let [bix (aget a (+ (* i 7) 0)) biy (aget a (+ (* i 7) 1)) biz (aget a (+ (* i 7) 2))
          bimass (aget a (+ (* i 7) 6))]
      (loop [j (inc i) vx (aget a (+ (* i 7) 3)) vy (aget a (+ (* i 7) 4)) vz (aget a (+ (* i 7) 5))]
        (if (< j 5)
          (let [dx (- bix (aget a (+ (* j 7) 0)))
                dy (- biy (aget a (+ (* j 7) 1)))
                dz (- biz (aget a (+ (* j 7) 2)))
                d2 (+ (* dx dx) (* dy dy) (* dz dz))
                mag (/ dt (* d2 (Math/sqrt d2)))
                jmass (aget a (+ (* j 7) 6))]
            (aset-double a (+ (* j 7) 3) (+ (aget a (+ (* j 7) 3)) (* dx bimass mag)))
            (aset-double a (+ (* j 7) 4) (+ (aget a (+ (* j 7) 4)) (* dy bimass mag)))
            (aset-double a (+ (* j 7) 5) (+ (aget a (+ (* j 7) 5)) (* dz bimass mag)))
            (recur (inc j) (- vx (* dx jmass mag)) (- vy (* dy jmass mag)) (- vz (* dz jmass mag))))
          (do
            (aset-double a (+ (* i 7) 3) vx)
            (aset-double a (+ (* i 7) 4) vy)
            (aset-double a (+ (* i 7) 5) vz))))))
  (dotimes [k 5]
    (aset-double a (+ (* k 7) 0) (+ (aget a (+ (* k 7) 0)) (* dt (aget a (+ (* k 7) 3)))))
    (aset-double a (+ (* k 7) 1) (+ (aget a (+ (* k 7) 1)) (* dt (aget a (+ (* k 7) 4)))))
    (aset-double a (+ (* k 7) 2) (+ (aget a (+ (* k 7) 2)) (* dt (aget a (+ (* k 7) 5)))))))

(defn nb-mut-energy [^doubles a]
  (loop [i 0 e 0.0]
    (if (< i 5)
      (let [vx (aget a (+ (* i 7) 3)) vy (aget a (+ (* i 7) 4)) vz (aget a (+ (* i 7) 5))
            e1 (+ e (* 0.5 (aget a (+ (* i 7) 6)) (+ (* vx vx) (* vy vy) (* vz vz))))
            e2 (loop [j (inc i) e e1]
                 (if (< j 5)
                   (let [dx (- (aget a (+ (* i 7) 0)) (aget a (+ (* j 7) 0)))
                         dy (- (aget a (+ (* i 7) 1)) (aget a (+ (* j 7) 1)))
                         dz (- (aget a (+ (* i 7) 2)) (aget a (+ (* j 7) 2)))
                         dist (Math/sqrt (+ (* dx dx) (* dy dy) (* dz dz)))]
                     (recur (inc j) (- e (/ (* (aget a (+ (* i 7) 6)) (aget a (+ (* j 7) 6))) dist))))
                   e))]
        (recur (inc i) e2))
      e)))

(defn nbody-mut-run [size]
  (let [a (nb-mut-init)]
    (dotimes [_ size] (nb-mut-advance a 0.01))
    (long (Math/round (* (nb-mut-energy a) 1e8)))))

(def nbody-mut-bench
  {:name "nbody_mut" :warmup 5 :iters 20 :size 20000 :expected -16908926 :run nbody-mut-run})
