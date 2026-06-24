#lang racket/base
;; Racket treelist mirror of boot/bench/slice_dropfirst.tw
;; Left-drop / dequeue: trim one element from the front each iteration via
;; treelist-drop. RRB slice makes this near-linear; a radix vector is O(n^2).
;;   racket boot/bench/racket/slice_dropfirst.rkt
(require racket/treelist)

(define (bench n)
  (define acc0
    (for/fold ([a (treelist)]) ([i (in-range n)])
      (treelist-add a i)))
  (define t0 (current-inexact-milliseconds))
  (let loop ([acc acc0] [sum 0])
    (cond
      [(> (treelist-length acc) 1)
       (loop (treelist-drop acc 1) (+ sum (treelist-ref acc 0)))]
      [else
       (define dt (- (current-inexact-milliseconds) t0))
       (printf "~a\t~a\t~a\n" n dt sum)])))

(printf "N\tms\tsink\n")
(for ([n (in-list '(1000 2000 4000 8000 16000))])
  (bench n))
