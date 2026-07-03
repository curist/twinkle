#lang racket/base
;; NBody over a Racket treelist of immutable body vectors. Each velocity/position
;; update rebinds the outer treelist with `treelist-set`. Constants and the
;; arithmetic order mirror the other ports exactly so the scaled-integer energy
;; checksum agrees bit-for-bit (flonums, no FMA fusion).
;;
;; Body layout: #(x y z vx vy vz mass).
(require racket/treelist racket/flonum "harness.rkt")
(provide nbody-bench)

(define nb-pi 3.141592653589793)
(define nb-solar-mass (* 4.0 nb-pi nb-pi))
(define nb-dpy 365.24)

(define (nb-init)
  (define bodies
    (treelist
     (vector 0.0 0.0 0.0 0.0 0.0 0.0 nb-solar-mass)
     (vector 4.84143144246472090 -1.16032004402742839 -1.03622044471123109e-01
             (* 1.66007664274403694e-03 nb-dpy) (* 7.69901118419740425e-03 nb-dpy) (* -6.90460016972063023e-05 nb-dpy)
             (* 9.54791938424326609e-04 nb-solar-mass))
     (vector 8.34336671824457987 4.12479856412430479 -4.03523417114321381e-01
             (* -2.76742510726862411e-03 nb-dpy) (* 4.99852801234917238e-03 nb-dpy) (* 2.30417297573763929e-05 nb-dpy)
             (* 2.85885980666130812e-04 nb-solar-mass))
     (vector 1.28943695621391310e+01 -1.51111514016986312e+01 -2.23307578892655734e-01
             (* 2.96460137564761618e-03 nb-dpy) (* 2.37847173959480950e-03 nb-dpy) (* -2.96589568540237556e-05 nb-dpy)
             (* 4.36624404335156298e-05 nb-solar-mass))
     (vector 1.53796971148509165e+01 -2.59193146099879641e+01 1.79258772950371181e-01
             (* 2.68067772490389322e-03 nb-dpy) (* 1.62824170038242295e-03 nb-dpy) (* -9.51592254519715870e-05 nb-dpy)
             (* 5.15138902046611451e-05 nb-solar-mass))))
  (define (msum sel)
    (let loop ([i 0] [acc 0.0])
      (if (< i (treelist-length bodies))
          (let ([b (treelist-ref bodies i)])
            (loop (add1 i) (+ acc (* (vector-ref b sel) (vector-ref b 6)))))
          acc)))
  (define px (msum 3)) (define py (msum 4)) (define pz (msum 5))
  (define s (treelist-ref bodies 0))
  (treelist-set bodies 0
    (vector (vector-ref s 0) (vector-ref s 1) (vector-ref s 2)
            (/ (- px) nb-solar-mass) (/ (- py) nb-solar-mass) (/ (- pz) nb-solar-mass)
            (vector-ref s 6))))

(define (nb-advance bodies dt)
  (define n (treelist-length bodies))
  (define bs1
    (let i-loop ([i 0] [bs bodies])
      (if (< i n)
          (let* ([bi (treelist-ref bs i)]
                 [bix (vector-ref bi 0)] [biy (vector-ref bi 1)] [biz (vector-ref bi 2)] [bimass (vector-ref bi 6)])
            (let j-loop ([j (add1 i)] [bs bs] [vx (vector-ref bi 3)] [vy (vector-ref bi 4)] [vz (vector-ref bi 5)])
              (if (< j n)
                  (let* ([bj (treelist-ref bs j)]
                         [dx (- bix (vector-ref bj 0))]
                         [dy (- biy (vector-ref bj 1))]
                         [dz (- biz (vector-ref bj 2))]
                         [d2 (+ (* dx dx) (* dy dy) (* dz dz))]
                         [mag (/ dt (* d2 (sqrt d2)))]
                         [bjmass (vector-ref bj 6)]
                         [bj2 (vector (vector-ref bj 0) (vector-ref bj 1) (vector-ref bj 2)
                                      (+ (vector-ref bj 3) (* dx bimass mag))
                                      (+ (vector-ref bj 4) (* dy bimass mag))
                                      (+ (vector-ref bj 5) (* dz bimass mag))
                                      bjmass)])
                    (j-loop (add1 j) (treelist-set bs j bj2)
                            (- vx (* dx bjmass mag)) (- vy (* dy bjmass mag)) (- vz (* dz bjmass mag))))
                  (i-loop (add1 i) (treelist-set bs i (vector bix biy biz vx vy vz bimass))))))
          bs)))
  (let k-loop ([k 0] [bs bs1])
    (if (< k n)
        (let ([b (treelist-ref bs k)])
          (k-loop (add1 k)
                  (treelist-set bs k
                    (vector (+ (vector-ref b 0) (* dt (vector-ref b 3)))
                            (+ (vector-ref b 1) (* dt (vector-ref b 4)))
                            (+ (vector-ref b 2) (* dt (vector-ref b 5)))
                            (vector-ref b 3) (vector-ref b 4) (vector-ref b 5) (vector-ref b 6)))))
        bs)))

(define (nb-energy bodies)
  (define n (treelist-length bodies))
  (let i-loop ([i 0] [e 0.0])
    (if (< i n)
        (let* ([bi (treelist-ref bodies i)]
               [e1 (+ e (* 0.5 (vector-ref bi 6)
                           (+ (* (vector-ref bi 3) (vector-ref bi 3))
                              (* (vector-ref bi 4) (vector-ref bi 4))
                              (* (vector-ref bi 5) (vector-ref bi 5)))))])
          (let j-loop ([j (add1 i)] [e e1])
            (if (< j n)
                (let* ([bj (treelist-ref bodies j)]
                       [dx (- (vector-ref bi 0) (vector-ref bj 0))]
                       [dy (- (vector-ref bi 1) (vector-ref bj 1))]
                       [dz (- (vector-ref bi 2) (vector-ref bj 2))]
                       [dist (sqrt (+ (* dx dx) (* dy dy) (* dz dz)))])
                  (j-loop (add1 j) (- e (/ (* (vector-ref bi 6) (vector-ref bj 6)) dist))))
                (i-loop (add1 i) e))))
        e)))

(define (nbody-run size)
  (define final
    (let loop ([i 0] [bodies (nb-init)])
      (if (< i size) (loop (add1 i) (nb-advance bodies 0.01)) bodies)))
  (inexact->exact (round (* (nb-energy final) 1e8))))

(define nbody-bench (bench "nbody" 5 20 20000 -16908926 nbody-run))

;; Unlocked tier: 5 bodies packed into a native flvector (unboxed flonums,
;; 7 fields each), mutated in place — Racket's escape hatch to the native
;; league. Body b, field f -> index (b*7 + f).
(provide nbody-mut-bench)
(define (nb-mut-init)
  (define a (make-flvector 35 0.0))
  (define (set-body b x y z vx vy vz mass)
    (flvector-set! a (+ (* b 7) 0) x) (flvector-set! a (+ (* b 7) 1) y)
    (flvector-set! a (+ (* b 7) 2) z) (flvector-set! a (+ (* b 7) 3) vx)
    (flvector-set! a (+ (* b 7) 4) vy) (flvector-set! a (+ (* b 7) 5) vz)
    (flvector-set! a (+ (* b 7) 6) mass))
  (set-body 0 0.0 0.0 0.0 0.0 0.0 0.0 nb-solar-mass)
  (set-body 1 4.84143144246472090 -1.16032004402742839 -1.03622044471123109e-01
            (* 1.66007664274403694e-03 nb-dpy) (* 7.69901118419740425e-03 nb-dpy) (* -6.90460016972063023e-05 nb-dpy)
            (* 9.54791938424326609e-04 nb-solar-mass))
  (set-body 2 8.34336671824457987 4.12479856412430479 -4.03523417114321381e-01
            (* -2.76742510726862411e-03 nb-dpy) (* 4.99852801234917238e-03 nb-dpy) (* 2.30417297573763929e-05 nb-dpy)
            (* 2.85885980666130812e-04 nb-solar-mass))
  (set-body 3 1.28943695621391310e+01 -1.51111514016986312e+01 -2.23307578892655734e-01
            (* 2.96460137564761618e-03 nb-dpy) (* 2.37847173959480950e-03 nb-dpy) (* -2.96589568540237556e-05 nb-dpy)
            (* 4.36624404335156298e-05 nb-solar-mass))
  (set-body 4 1.53796971148509165e+01 -2.59193146099879641e+01 1.79258772950371181e-01
            (* 2.68067772490389322e-03 nb-dpy) (* 1.62824170038242295e-03 nb-dpy) (* -9.51592254519715870e-05 nb-dpy)
            (* 5.15138902046611451e-05 nb-solar-mass))
  (define (msum sel)
    (let loop ([b 0] [acc 0.0])
      (if (< b 5) (loop (add1 b) (+ acc (* (flvector-ref a (+ (* b 7) sel)) (flvector-ref a (+ (* b 7) 6))))) acc)))
  (flvector-set! a 3 (/ (- (msum 3)) nb-solar-mass))
  (flvector-set! a 4 (/ (- (msum 4)) nb-solar-mass))
  (flvector-set! a 5 (/ (- (msum 5)) nb-solar-mass))
  a)

(define (nb-mut-advance a dt)
  (let i-loop ([i 0])
    (when (< i 5)
      (define bix (flvector-ref a (+ (* i 7) 0)))
      (define biy (flvector-ref a (+ (* i 7) 1)))
      (define biz (flvector-ref a (+ (* i 7) 2)))
      (define bimass (flvector-ref a (+ (* i 7) 6)))
      (let j-loop ([j (add1 i)]
                   [vx (flvector-ref a (+ (* i 7) 3))]
                   [vy (flvector-ref a (+ (* i 7) 4))]
                   [vz (flvector-ref a (+ (* i 7) 5))])
        (if (< j 5)
            (let* ([dx (- bix (flvector-ref a (+ (* j 7) 0)))]
                   [dy (- biy (flvector-ref a (+ (* j 7) 1)))]
                   [dz (- biz (flvector-ref a (+ (* j 7) 2)))]
                   [d2 (+ (* dx dx) (* dy dy) (* dz dz))]
                   [mag (/ dt (* d2 (sqrt d2)))]
                   [jmass (flvector-ref a (+ (* j 7) 6))])
              (flvector-set! a (+ (* j 7) 3) (+ (flvector-ref a (+ (* j 7) 3)) (* dx bimass mag)))
              (flvector-set! a (+ (* j 7) 4) (+ (flvector-ref a (+ (* j 7) 4)) (* dy bimass mag)))
              (flvector-set! a (+ (* j 7) 5) (+ (flvector-ref a (+ (* j 7) 5)) (* dz bimass mag)))
              (j-loop (add1 j) (- vx (* dx jmass mag)) (- vy (* dy jmass mag)) (- vz (* dz jmass mag))))
            (begin
              (flvector-set! a (+ (* i 7) 3) vx)
              (flvector-set! a (+ (* i 7) 4) vy)
              (flvector-set! a (+ (* i 7) 5) vz)
              (i-loop (add1 i)))))))
  (let k-loop ([k 0])
    (when (< k 5)
      (flvector-set! a (+ (* k 7) 0) (+ (flvector-ref a (+ (* k 7) 0)) (* dt (flvector-ref a (+ (* k 7) 3)))))
      (flvector-set! a (+ (* k 7) 1) (+ (flvector-ref a (+ (* k 7) 1)) (* dt (flvector-ref a (+ (* k 7) 4)))))
      (flvector-set! a (+ (* k 7) 2) (+ (flvector-ref a (+ (* k 7) 2)) (* dt (flvector-ref a (+ (* k 7) 5)))))
      (k-loop (add1 k)))))

(define (nb-mut-energy a)
  (let i-loop ([i 0] [e 0.0])
    (if (< i 5)
        (let* ([vx (flvector-ref a (+ (* i 7) 3))] [vy (flvector-ref a (+ (* i 7) 4))] [vz (flvector-ref a (+ (* i 7) 5))]
               [e1 (+ e (* 0.5 (flvector-ref a (+ (* i 7) 6)) (+ (* vx vx) (* vy vy) (* vz vz))))])
          (let j-loop ([j (add1 i)] [e e1])
            (if (< j 5)
                (let* ([dx (- (flvector-ref a (+ (* i 7) 0)) (flvector-ref a (+ (* j 7) 0)))]
                       [dy (- (flvector-ref a (+ (* i 7) 1)) (flvector-ref a (+ (* j 7) 1)))]
                       [dz (- (flvector-ref a (+ (* i 7) 2)) (flvector-ref a (+ (* j 7) 2)))]
                       [dist (sqrt (+ (* dx dx) (* dy dy) (* dz dz)))])
                  (j-loop (add1 j) (- e (/ (* (flvector-ref a (+ (* i 7) 6)) (flvector-ref a (+ (* j 7) 6))) dist))))
                (i-loop (add1 i) e))))
        e)))

(define (nbody-mut-run size)
  (define a (nb-mut-init))
  (let loop ([i 0]) (when (< i size) (nb-mut-advance a 0.01) (loop (add1 i))))
  (inexact->exact (round (* (nb-mut-energy a) 1e8))))
(define nbody-mut-bench (bench "nbody_mut" 5 20 20000 -16908926 nbody-mut-run))
