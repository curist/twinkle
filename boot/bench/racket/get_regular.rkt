#lang racket/base
;; Racket treelist mirror of boot/bench/get_regular.tw
;; N strided treelist-ref on an append-built (fully regular) treelist. Build is
;; outside the timed region; a large coprime stride defeats cache locality.
;;   racket boot/bench/racket/get_regular.rkt
(require racket/treelist)

(define (bench n)
  (define v
    (for/fold ([a (treelist)]) ([i (in-range n)])
      (treelist-add a i)))
  (define t0 (current-inexact-milliseconds))
  (let loop ([k 0] [idx 0] [sum 0])
    (cond
      [(= k n)
       (define dt (- (current-inexact-milliseconds) t0))
       (printf "~a\t~a\t~a\n" n dt sum)]
      [else
       (define nidx (modulo (+ idx 40503) n))
       (loop (add1 k) nidx (+ sum (treelist-ref v nidx)))])))

(printf "N\tms\tsink\n")
(for ([n (in-list '(1000 2000 4000 8000 16000 32000))])
  (bench n))
