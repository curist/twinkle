(module
  (type $rt_types__Array (array (mut anyref)))
  (type $rt_types__String (array (mut i8)))
  (type $rt_types__DictEntry (struct (field $key anyref) (field $val anyref)))
  (type $rt_types__Dict (array (mut (ref null $rt_types__DictEntry))))
  (type $rt_types__ClosureEnv (array anyref))
  (type $rt_types__ClosureFunc (func (param anyref anyref) (result anyref)))
  (type $rt_types__Closure (sub (struct (field $func_ref (ref null $rt_types__ClosureFunc)) (field $env (ref null $rt_types__ClosureEnv)))))
  (type $rt_types__Variant (struct (field $type_id i32) (field $variant_id i32) (field $payload (ref null $rt_types__Array))))
  (type $rt_types__BoxedInt (struct (field $v i64)))
  (type $rt_types__BoxedFloat (struct (field $v f64)))
  (type $rt_types__IterState (sub (struct (field $seed anyref) (field $step anyref))))
  (type $user__unfold_step__Int__Int (struct (field $variant_id i32) (field $f0 i64) (field $f1 i64)))
  (type $user__unfold_step__String__Int (struct (field $variant_id i32) (field $f0 (ref null $rt_types__String)) (field $f1 i64)))
  (type $user__closurefunc_i64_T6_Int_Int (func (param (ref null $rt_types__ClosureEnv) i64) (result (ref null $rt_types__Variant))))
  (type $user__closurefunc_i64_T6_String_Int (func (param (ref null $rt_types__ClosureEnv) i64) (result (ref null $rt_types__Variant))))
  (type $user__UserRecord_2 (struct))
  (type $user__UserRecord_3 (struct (field $f0 (mut i64)) (field $f1 (mut i64)) (field $f2 (mut i64))))
  (type $user__UserRecord_4 (struct))
  (type $user__UserRecord_5 (struct (field $f0 (mut anyref)) (field $f1 (mut (ref null $rt_types__IterState)))))
  (type $user__closure_i64_T6_Int_Int (sub $rt_types__Closure (struct (field $func_ref (ref null $rt_types__ClosureFunc)) (field $env (ref null $rt_types__ClosureEnv)) (field $typed_ref (ref null $user__closurefunc_i64_T6_Int_Int)))))
  (type $user__closure_i64_T6_String_Int (sub $rt_types__Closure (struct (field $func_ref (ref null $rt_types__ClosureFunc)) (field $env (ref null $rt_types__ClosureEnv)) (field $typed_ref (ref null $user__closurefunc_i64_T6_String_Int)))))
  (type $user__iter_state__Int__Int (struct (field $seed i64) (field $step (ref null $user__closure_i64_T6_Int_Int))))
  (type $user__iter_state__String__Int (struct (field $seed i64) (field $step (ref null $user__closure_i64_T6_String_Int))))
  (type $functype_0 (func (param f64) (result (ref $rt_types__String))))
  (type $functype_1 (func (param (ref null $rt_types__String))))
  (type $functype_2 (func (param i32 anyref) (result (ref $rt_types__Array))))
  (type $functype_3 (func (param (ref null $rt_types__Array) i32) (result anyref)))
  (type $functype_4 (func (param (ref null $rt_types__Array) i32 anyref) (result (ref $rt_types__Array))))
  (type $functype_5 (func (param (ref null $rt_types__Array)) (result i32)))
  (type $functype_6 (func (param (ref null $rt_types__Array) (ref null $rt_types__Array)) (result (ref $rt_types__Array))))
  (type $functype_7 (func (param (ref null $rt_types__Array) i32 i32) (result (ref $rt_types__Array))))
  (type $functype_8 (func (result (ref $rt_types__Array))))
  (type $functype_9 (func (param (ref null $rt_types__Array)) (result (ref $rt_types__Array))))
  (type $functype_10 (func (param (ref null $rt_types__Array) anyref)))
  (type $functype_11 (func (param (ref null $rt_types__String)) (result i32)))
  (type $functype_12 (func (param (ref null $rt_types__String) (ref null $rt_types__String)) (result (ref $rt_types__String))))
  (type $functype_13 (func (param (ref null $rt_types__String) i32 i32) (result (ref $rt_types__String))))
  (type $functype_14 (func (param (ref null $rt_types__String) (ref null $rt_types__String)) (result i32)))
  (type $functype_15 (func (param i64) (result (ref $rt_types__String))))
  (type $functype_16 (func (param i32) (result (ref $rt_types__String))))
  (type $functype_17 (func (result (ref $rt_types__Dict))))
  (type $functype_18 (func (param (ref null $rt_types__Dict)) (result i32)))
  (type $functype_19 (func (param (ref null $rt_types__Dict)) (result (ref $rt_types__Array))))
  (type $functype_20 (func (param (ref null $rt_types__Dict) anyref) (result i32)))
  (type $functype_21 (func (param (ref null $rt_types__Dict) anyref) (result anyref)))
  (type $functype_22 (func (param (ref null $rt_types__Dict) anyref) (result (ref $rt_types__Variant))))
  (type $functype_23 (func (param (ref null $rt_types__Dict) anyref anyref) (result (ref $rt_types__Dict))))
  (type $functype_24 (func (param (ref null $rt_types__Dict) anyref) (result (ref $rt_types__Dict))))
  (type $functype_25 (func (param anyref anyref) (result i32)))
  (type $functype_26 (func (param (ref null $rt_types__String)) (result (ref null $rt_types__IterState))))
  (type $functype_27 (func (param i64) (result (ref null $rt_types__IterState))))
  (type $functype_28 (func (param (ref null $rt_types__String) i64 i64) (result (ref null $rt_types__String))))
  (type $functype_29 (func (param (ref null $rt_types__Array)) (result (ref null $rt_types__IterState))))
  (type $functype_30 (func))
  (type $functype_31 (func (param i64 anyref) (result (ref null $rt_types__Variant))))
  (type $functype_32 (func (param i64 anyref anyref) (result (ref null $rt_types__Variant))))
  (type $functype_33 (func (param anyref anyref) (result anyref)))
  (type $functype_34 (func (param (ref null $rt_types__ClosureEnv) i64) (result (ref null $rt_types__Variant))))
  (type $functype_35 (func (param anyref) (result (ref null $rt_types__Variant))))
  (type $functype_36 (func (param (ref null $rt_types__String)) (result anyref)))
  (type $functype_37 (func (param i32) (result anyref)))
  (type $functype_38 (func (param (ref null $rt_types__String)) (result (ref $rt_types__Array))))
  (type $functype_39 (func (param (ref null $rt_types__Array)) (result anyref)))
  (type $functype_40 (func (result (ref $rt_types__String))))
  (import "host" "f64_to_string" (func $rt_str__host_f64_to_string (type $functype_0)))
  (import "host" "print" (func $rt_core__host_print (type $functype_1)))
  (import "host" "println" (func $rt_core__host_println (type $functype_1)))
  (import "host" "error" (func $rt_core__host_error (type $functype_1)))
  (import "host" "eprint" (func $rt_core__host_eprint (type $functype_1)))
  (import "host" "eprintln" (func $rt_core__host_eprintln (type $functype_1)))
  (global $user__global_local_0 (mut anyref) (ref.null none))
  (global $user__global_local_1 (mut anyref) (ref.null none))
  (global $user____str_lit_global_empty (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_20 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_3d (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_48656c6c6f (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_5f (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_63686172733a (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_63756d756c3a (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_6974656d (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_6c6162656c733a (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (global $user____str_lit_global_6e6f6e2d65786861757374697665206d61746368 (mut (ref null $rt_types__String)) (ref.null $rt_types__String))
  (func $rt_arr__make (type $functype_2)
    (param $p0 i32)
    (param $p1 anyref)
    (result (ref $rt_types__Array))
    local.get $p1
    local.get $p0
    array.new $rt_types__Array
  )
  (func $rt_arr__get (type $functype_3)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 i32)
    (result anyref)
    local.get $p0
    ref.as_non_null
    local.get $p1
    array.get $rt_types__Array
  )
  (func $rt_arr__set (type $functype_4)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 i32)
    (param $p2 anyref)
    (result (ref $rt_types__Array))
    (local $p3 (ref null $rt_types__Array))
    (local $p4 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p4
    ref.null none
    local.get $p4
    array.new $rt_types__Array
    local.set $p3
    local.get $p3
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    i32.const 0
    local.get $p4
    array.copy $rt_types__Array $rt_types__Array
    local.get $p3
    ref.as_non_null
    local.get $p1
    local.get $p2
    array.set $rt_types__Array
    local.get $p3
    ref.as_non_null
  )
  (func $rt_arr__len (type $functype_5)
    (param $p0 (ref null $rt_types__Array))
    (result i32)
    local.get $p0
    ref.as_non_null
    array.len
  )
  (func $rt_arr__concat (type $functype_6)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 (ref null $rt_types__Array))
    (result (ref $rt_types__Array))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    local.get $p1
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p2
    local.get $p3
    i32.add
    local.set $p4
    ref.null none
    local.get $p4
    array.new $rt_types__Array
    local.set $p5
    local.get $p5
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    i32.const 0
    local.get $p2
    array.copy $rt_types__Array $rt_types__Array
    local.get $p5
    ref.as_non_null
    local.get $p2
    local.get $p1
    ref.as_non_null
    i32.const 0
    local.get $p3
    array.copy $rt_types__Array $rt_types__Array
    local.get $p5
    ref.as_non_null
  )
  (func $rt_arr__slice (type $functype_7)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 i32)
    (param $p2 i32)
    (result (ref $rt_types__Array))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__Array))
    local.get $p2
    local.get $p1
    i32.sub
    local.set $p3
    ref.null none
    local.get $p3
    array.new $rt_types__Array
    local.set $p4
    local.get $p4
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    local.get $p1
    local.get $p3
    array.copy $rt_types__Array $rt_types__Array
    local.get $p4
    ref.as_non_null
  )
  (func $rt_arr__builder_new (type $functype_8)
    (result (ref $rt_types__Array))
    ref.null none
    i32.const 8
    array.new $rt_types__Array
    i64.const 0
    struct.new $rt_types__BoxedInt
    i64.const 8
    struct.new $rt_types__BoxedInt
    array.new_fixed $rt_types__Array 3
  )
  (func $rt_arr__builder_from (type $functype_9)
    (param $p0 (ref null $rt_types__Array))
    (result (ref $rt_types__Array))
    (local $p1 i32)
    (local $p2 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p1
    local.get $p1
    local.set $p2
    local.get $p0
    ref.as_non_null
    local.get $p1
    i64.extend_i32_s
    struct.new $rt_types__BoxedInt
    local.get $p2
    i64.extend_i32_s
    struct.new $rt_types__BoxedInt
    array.new_fixed $rt_types__Array 3
  )
  (func $rt_arr__builder_push (type $functype_10)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 anyref)
    (local $p2 (ref null $rt_types__Array))
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__Array)
    local.set $p2
    local.get $p0
    ref.as_non_null
    i32.const 1
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    i32.wrap_i64
    local.set $p3
    local.get $p0
    ref.as_non_null
    i32.const 2
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    i32.wrap_i64
    local.set $p4
    local.get $p3
    local.get $p4
    i32.lt_s
    (if
      (then
        local.get $p2
        ref.as_non_null
        local.get $p3
        local.get $p1
        array.set $rt_types__Array
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        local.get $p0
        ref.as_non_null
        i32.const 1
        local.get $p3
        i64.extend_i32_s
        struct.new $rt_types__BoxedInt
        array.set $rt_types__Array)
      (else
        local.get $p4
        i32.eqz
        (if (result i32)
          (then
            i32.const 8)
          (else
            local.get $p4
            i32.const 2
            i32.mul))
        local.set $p5
        ref.null none
        local.get $p5
        array.new $rt_types__Array
        local.set $p6
        local.get $p6
        ref.as_non_null
        i32.const 0
        local.get $p2
        ref.as_non_null
        i32.const 0
        local.get $p3
        array.copy $rt_types__Array $rt_types__Array
        local.get $p6
        ref.as_non_null
        local.get $p3
        local.get $p1
        array.set $rt_types__Array
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        local.get $p5
        local.set $p4
        local.get $p0
        ref.as_non_null
        i32.const 0
        local.get $p6
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        i32.const 1
        local.get $p3
        i64.extend_i32_s
        struct.new $rt_types__BoxedInt
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        i32.const 2
        local.get $p4
        i64.extend_i32_s
        struct.new $rt_types__BoxedInt
        array.set $rt_types__Array))
  )
  (func $rt_arr__builder_freeze (type $functype_9)
    (param $p0 (ref null $rt_types__Array))
    (result (ref $rt_types__Array))
    (local $p1 (ref null $rt_types__Array))
    (local $p2 i32)
    (local $p3 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__Array)
    local.set $p1
    local.get $p0
    ref.as_non_null
    i32.const 1
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    i32.wrap_i64
    local.set $p2
    ref.null none
    local.get $p2
    array.new $rt_types__Array
    local.set $p3
    local.get $p3
    ref.as_non_null
    i32.const 0
    local.get $p1
    ref.as_non_null
    i32.const 0
    local.get $p2
    array.copy $rt_types__Array $rt_types__Array
    local.get $p3
    ref.as_non_null
  )
  (func $rt_str__len (type $functype_11)
    (param $p0 (ref null $rt_types__String))
    (result i32)
    local.get $p0
    ref.as_non_null
    array.len
  )
  (func $rt_str__concat (type $functype_12)
    (param $p0 (ref null $rt_types__String))
    (param $p1 (ref null $rt_types__String))
    (result (ref $rt_types__String))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__String))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    local.get $p1
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p2
    local.get $p3
    i32.add
    local.set $p4
    i32.const 0
    local.get $p4
    array.new $rt_types__String
    local.set $p5
    local.get $p5
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    i32.const 0
    local.get $p2
    array.copy $rt_types__String $rt_types__String
    local.get $p5
    ref.as_non_null
    local.get $p2
    local.get $p1
    ref.as_non_null
    i32.const 0
    local.get $p3
    array.copy $rt_types__String $rt_types__String
    local.get $p5
    ref.as_non_null
  )
  (func $rt_str__substring (type $functype_13)
    (param $p0 (ref null $rt_types__String))
    (param $p1 i32)
    (param $p2 i32)
    (result (ref $rt_types__String))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__String))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p1
    i32.const 0
    i32.lt_s
    (if
      (then
        local.get $p3
        local.set $p1))
    local.get $p1
    local.get $p3
    i32.gt_s
    (if
      (then
        local.get $p3
        local.set $p1))
    local.get $p2
    i32.const 0
    i32.lt_s
    (if
      (then
        local.get $p3
        local.set $p2))
    local.get $p2
    local.get $p3
    i32.gt_s
    (if
      (then
        local.get $p3
        local.set $p2))
    local.get $p2
    local.get $p1
    i32.lt_s
    (if
      (then
        local.get $p1
        local.set $p2))
    local.get $p2
    local.get $p1
    i32.sub
    local.set $p3
    i32.const 0
    local.get $p3
    array.new $rt_types__String
    local.set $p4
    local.get $p4
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    local.get $p1
    local.get $p3
    array.copy $rt_types__String $rt_types__String
    local.get $p4
    ref.as_non_null
  )
  (func $rt_str__eq (type $functype_14)
    (param $p0 (ref null $rt_types__String))
    (param $p1 (ref null $rt_types__String))
    (result i32)
    (local $p2 i32)
    (local $p3 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    local.get $p2
    local.get $p1
    ref.as_non_null
    array.len
    i32.ne
    (if
      (then
        i32.const 0
        return))
    i32.const 0
    local.set $p3
    (block $exit
      (loop $cmp
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        local.get $p1
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        i32.ne
        (if
          (then
            i32.const 0
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $cmp))
    i32.const 1
  )
  (func $rt_str__cmp (type $functype_14)
    (param $p0 (ref null $rt_types__String))
    (param $p1 (ref null $rt_types__String))
    (result i32)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 i32)
    (local $p7 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p6
    local.get $p1
    ref.as_non_null
    array.len
    local.set $p7
    local.get $p6
    local.get $p7
    local.get $p6
    local.get $p7
    i32.le_s
    select
    local.set $p2
    i32.const 0
    local.set $p3
    (block $done
      (loop $cmp_loop
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $done
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        local.set $p4
        local.get $p1
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        local.set $p5
        local.get $p4
        local.get $p5
        i32.lt_u
        (if
          (then
            i32.const -1
            return))
        local.get $p4
        local.get $p5
        i32.gt_u
        (if
          (then
            i32.const 1
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $cmp_loop))
    local.get $p6
    local.get $p7
    i32.lt_s
    (if
      (then
        i32.const -1
        return))
    local.get $p6
    local.get $p7
    i32.gt_s
    (if
      (then
        i32.const 1
        return))
    i32.const 0
  )
  (func $rt_str__from_i64 (type $functype_15)
    (param $p0 i64)
    (result (ref $rt_types__String))
    (local $p1 i32)
    (local $p2 i64)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__String))
    (local $p6 (ref null $rt_types__String))
    local.get $p0
    i64.eqz
    (if
      (then
        i32.const 48
        array.new_fixed $rt_types__String 1
        return))
    local.get $p0
    i64.const -9223372036854775808
    i64.eq
    (if
      (then
        i32.const 45
        i32.const 57
        i32.const 50
        i32.const 50
        i32.const 51
        i32.const 51
        i32.const 55
        i32.const 50
        i32.const 48
        i32.const 51
        i32.const 54
        i32.const 56
        i32.const 53
        i32.const 52
        i32.const 55
        i32.const 55
        i32.const 53
        i32.const 56
        i32.const 48
        i32.const 56
        array.new_fixed $rt_types__String 20
        return))
    local.get $p0
    i64.const 0
    i64.lt_s
    local.set $p1
    local.get $p1
    (if (result i64)
      (then
        i64.const 0
        local.get $p0
        i64.sub)
      (else
        local.get $p0))
    local.set $p2
    i32.const 0
    i32.const 20
    array.new $rt_types__String
    local.set $p5
    i32.const 19
    local.set $p3
    (loop $digits
      local.get $p5
      ref.as_non_null
      local.get $p3
      local.get $p2
      i64.const 10
      i64.rem_s
      i32.wrap_i64
      i32.const 48
      i32.add
      array.set $rt_types__String
      local.get $p2
      i64.const 10
      i64.div_s
      local.set $p2
      local.get $p3
      i32.const 1
      i32.sub
      local.set $p3
      local.get $p2
      i64.eqz
      i32.eqz
      br_if $digits)
    local.get $p1
    (if
      (then
        local.get $p5
        ref.as_non_null
        local.get $p3
        i32.const 45
        array.set $rt_types__String
        local.get $p3
        i32.const 1
        i32.sub
        local.set $p3))
    i32.const 19
    local.get $p3
    i32.sub
    local.set $p4
    i32.const 0
    local.get $p4
    array.new $rt_types__String
    local.set $p6
    local.get $p6
    ref.as_non_null
    i32.const 0
    local.get $p5
    ref.as_non_null
    local.get $p3
    i32.const 1
    i32.add
    local.get $p4
    array.copy $rt_types__String $rt_types__String
    local.get $p6
    ref.as_non_null
  )
  (func $rt_str__from_f64 (type $functype_0)
    (param $p0 f64)
    (result (ref $rt_types__String))
    local.get $p0
    call $rt_str__host_f64_to_string
  )
  (func $rt_str__from_bool (type $functype_16)
    (param $p0 i32)
    (result (ref $rt_types__String))
    local.get $p0
    (if (result (ref $rt_types__String))
      (then
        i32.const 116
        i32.const 114
        i32.const 117
        i32.const 101
        array.new_fixed $rt_types__String 4)
      (else
        i32.const 102
        i32.const 97
        i32.const 108
        i32.const 115
        i32.const 101
        array.new_fixed $rt_types__String 5))
  )
  (func $rt_dict__make (type $functype_17)
    (result (ref $rt_types__Dict))
    array.new_fixed $rt_types__Dict 0
  )
  (func $rt_dict__len (type $functype_18)
    (param $p0 (ref null $rt_types__Dict))
    (result i32)
    local.get $p0
    ref.as_non_null
    array.len
  )
  (func $rt_dict__keys (type $functype_19)
    (param $p0 (ref null $rt_types__Dict))
    (result (ref $rt_types__Array))
    (local $p1 i32)
    (local $p2 i32)
    (local $p3 (ref null $rt_types__Array))
    (local $p4 (ref null $rt_types__DictEntry))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p1
    ref.null none
    local.get $p1
    array.new $rt_types__Array
    local.set $p3
    i32.const 0
    local.set $p2
    (block $exit
      (loop $loop
        local.get $p2
        local.get $p1
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p2
        array.get $rt_types__Dict
        local.set $p4
        local.get $p3
        ref.as_non_null
        local.get $p2
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        array.set $rt_types__Array
        local.get $p2
        i32.const 1
        i32.add
        local.set $p2
        br $loop))
    local.get $p3
    ref.as_non_null
  )
  (func $rt_dict__has (type $functype_20)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (result i32)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__DictEntry))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    i32.const 0
    local.set $p3
    (block $exit
      (loop $scan
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p4
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            i32.const 1
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan))
    i32.const 0
  )
  (func $rt_dict__get (type $functype_21)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (result anyref)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__DictEntry))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    i32.const 0
    local.set $p3
    (block $exit
      (loop $scan
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p4
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            local.get $p4
            ref.as_non_null
            struct.get $rt_types__DictEntry 1
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan))
    ref.null any
  )
  (func $rt_dict__get_option (type $functype_22)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (result (ref $rt_types__Variant))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__DictEntry))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    i32.const 0
    local.set $p3
    (block $exit
      (loop $scan
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p4
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            i32.const 0
            i32.const 1
            local.get $p4
            ref.as_non_null
            struct.get $rt_types__DictEntry 1
            array.new_fixed $rt_types__Array 1
            struct.new $rt_types__Variant
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan))
    i32.const 0
    i32.const 0
    ref.null $rt_types__Array
    struct.new $rt_types__Variant
  )
  (func $rt_dict__set (type $functype_23)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (param $p2 anyref)
    (result (ref $rt_types__Dict))
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 (ref null $rt_types__Dict))
    (local $p7 (ref $rt_types__DictEntry))
    (local $p8 (ref null $rt_types__DictEntry))
    (local $p9 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p1
    local.get $p2
    struct.new $rt_types__DictEntry
    local.set $p7
    i32.const 0
    local.set $p4
    i32.const 0
    local.set $p5
    (block $found_exit
      (loop $scan
        local.get $p4
        local.get $p3
        i32.ge_s
        br_if $found_exit
        local.get $p0
        ref.as_non_null
        local.get $p4
        array.get $rt_types__Dict
        local.set $p8
        local.get $p8
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            i32.const 1
            local.set $p5
            br $found_exit))
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $scan))
    local.get $p5
    (if (result i32)
      (then
        local.get $p3)
      (else
        local.get $p3
        i32.const 1
        i32.add))
    local.set $p9
    ref.null $rt_types__DictEntry
    local.get $p9
    array.new $rt_types__Dict
    local.set $p6
    i32.const 0
    local.set $p4
    (block $copy_exit
      (loop $copy
        local.get $p4
        local.get $p3
        i32.ge_s
        br_if $copy_exit
        local.get $p0
        ref.as_non_null
        local.get $p4
        array.get $rt_types__Dict
        local.set $p8
        local.get $p8
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            local.get $p6
            ref.as_non_null
            local.get $p4
            local.get $p7
            array.set $rt_types__Dict)
          (else
            local.get $p6
            ref.as_non_null
            local.get $p4
            local.get $p8
            array.set $rt_types__Dict))
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $copy))
    local.get $p5
    i32.eqz
    (if
      (then
        local.get $p6
        ref.as_non_null
        local.get $p3
        local.get $p7
        array.set $rt_types__Dict))
    local.get $p6
    ref.as_non_null
  )
  (func $rt_dict__remove (type $functype_24)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (result (ref $rt_types__Dict))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__Dict))
    (local $p6 (ref null $rt_types__DictEntry))
    (local $p7 i32)
    (local $p8 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    i32.const 0
    local.set $p3
    i32.const 0
    local.set $p7
    (block $scan_exit
      (loop $scan
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $scan_exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p6
        local.get $p6
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            i32.const 1
            local.set $p7
            br $scan_exit))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan))
    local.get $p7
    (if (result i32)
      (then
        local.get $p2
        i32.const 1
        i32.sub)
      (else
        local.get $p2))
    local.set $p8
    ref.null $rt_types__DictEntry
    local.get $p8
    array.new $rt_types__Dict
    local.set $p5
    i32.const 0
    local.set $p3
    i32.const 0
    local.set $p4
    (block $copy_exit
      (loop $copy
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $copy_exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p6
        local.get $p6
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then)
          (else
            local.get $p5
            ref.as_non_null
            local.get $p4
            local.get $p6
            array.set $rt_types__Dict
            local.get $p4
            i32.const 1
            i32.add
            local.set $p4))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $copy))
    local.get $p5
    ref.as_non_null
  )
  (func $rt_dict__set_in_place (type $functype_23)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (param $p2 anyref)
    (result (ref $rt_types__Dict))
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__DictEntry))
    (local $p6 (ref null $rt_types__Dict))
    (local $p7 (ref $rt_types__DictEntry))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p1
    local.get $p2
    struct.new $rt_types__DictEntry
    local.set $p7
    i32.const 0
    local.set $p4
    (block $not_found
      (loop $scan
        local.get $p4
        local.get $p3
        i32.ge_s
        br_if $not_found
        local.get $p0
        ref.as_non_null
        local.get $p4
        array.get $rt_types__Dict
        local.set $p5
        local.get $p5
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            local.get $p0
            ref.as_non_null
            local.get $p4
            local.get $p7
            array.set $rt_types__Dict
            local.get $p0
            ref.as_non_null
            return))
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $scan))
    ref.null $rt_types__DictEntry
    local.get $p3
    i32.const 1
    i32.add
    array.new $rt_types__Dict
    local.set $p6
    local.get $p6
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    i32.const 0
    local.get $p3
    array.copy $rt_types__Dict $rt_types__Dict
    local.get $p6
    ref.as_non_null
    local.get $p3
    local.get $p7
    array.set $rt_types__Dict
    local.get $p6
    ref.as_non_null
  )
  (func $rt_dict__remove_in_place (type $functype_24)
    (param $p0 (ref null $rt_types__Dict))
    (param $p1 anyref)
    (result (ref $rt_types__Dict))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__DictEntry))
    (local $p6 (ref null $rt_types__Dict))
    (local $p7 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    i32.const 0
    local.set $p3
    i32.const -1
    local.set $p4
    (block $scan_exit
      (loop $scan
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $scan_exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Dict
        local.set $p5
        local.get $p5
        ref.as_non_null
        struct.get $rt_types__DictEntry 0
        local.get $p1
        call $rt_core__eq
        (if
          (then
            local.get $p3
            local.set $p4
            br $scan_exit))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan))
    local.get $p4
    i32.const 1
    i32.add
    i32.eqz
    (if
      (then
        local.get $p0
        ref.as_non_null
        return))
    ref.null $rt_types__DictEntry
    local.get $p2
    i32.const 1
    i32.sub
    array.new $rt_types__Dict
    local.set $p6
    local.get $p4
    i32.const 0
    i32.gt_s
    (if
      (then
        local.get $p6
        ref.as_non_null
        i32.const 0
        local.get $p0
        ref.as_non_null
        i32.const 0
        local.get $p4
        array.copy $rt_types__Dict $rt_types__Dict))
    local.get $p2
    local.get $p4
    i32.sub
    i32.const 1
    i32.sub
    local.set $p7
    local.get $p7
    i32.const 0
    i32.gt_s
    (if
      (then
        local.get $p6
        ref.as_non_null
        local.get $p4
        local.get $p0
        ref.as_non_null
        local.get $p4
        i32.const 1
        i32.add
        local.get $p7
        array.copy $rt_types__Dict $rt_types__Dict))
    local.get $p6
    ref.as_non_null
  )
  (func $rt_core__print (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_print
  )
  (func $rt_core__println (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_println
  )
  (func $rt_core__eprint (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_eprint
  )
  (func $rt_core__eprintln (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_eprintln
  )
  (func $rt_core__trap (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_error
    unreachable
  )
  (func $rt_core__eq (type $functype_25)
    (param $p0 anyref)
    (param $p1 anyref)
    (result i32)
    local.get $p0
    ref.cast (ref null eq)
    local.get $p1
    ref.cast (ref null eq)
    ref.eq
    (if
      (then
        i32.const 1
        return))
    local.get $p0
    ref.test (ref $rt_types__BoxedInt)
    local.get $p1
    ref.test (ref $rt_types__BoxedInt)
    i32.and
    (if
      (then
        local.get $p0
        ref.cast (ref $rt_types__BoxedInt)
        struct.get $rt_types__BoxedInt 0
        local.get $p1
        ref.cast (ref $rt_types__BoxedInt)
        struct.get $rt_types__BoxedInt 0
        i64.eq
        return))
    local.get $p0
    ref.test (ref $rt_types__String)
    local.get $p1
    ref.test (ref $rt_types__String)
    i32.and
    (if
      (then
        local.get $p0
        ref.cast (ref $rt_types__String)
        local.get $p1
        ref.cast (ref $rt_types__String)
        call $rt_str__eq
        return))
    local.get $p0
    ref.cast (ref null eq)
    local.get $p1
    ref.cast (ref null eq)
    ref.eq
  )
  (func $user__func_41 (type $functype_26)
    (param $p0 (ref null $rt_types__String))
    (result (ref null $rt_types__IterState))
    (local $p1 (ref null $user__closure_i64_T6_String_Int))
    (local $p2 (ref null $user__iter_state__String__Int))
    ref.func $user__func_45__closure
    local.get $p0
    array.new_fixed $rt_types__ClosureEnv 1
    ref.func $user__func_45__typed_closure
    struct.new $user__closure_i64_T6_String_Int
    local.set $p1
    i64.const 0
    local.get $p1
    struct.new $user__iter_state__String__Int
    local.set $p2
    local.get $p2
    struct.get $user__iter_state__String__Int 0
    struct.new $rt_types__BoxedInt
    local.get $p2
    struct.get $user__iter_state__String__Int 1
    struct.new $rt_types__IterState
    return
  )
  (func $user__func_42 (type $functype_27)
    (param $p0 i64)
    (result (ref null $rt_types__IterState))
    (local $p1 (ref null $user__closure_i64_T6_Int_Int))
    (local $p2 (ref null $user__iter_state__Int__Int))
    ref.func $user__func_46__closure
    local.get $p0
    struct.new $rt_types__BoxedInt
    array.new_fixed $rt_types__ClosureEnv 1
    ref.func $user__func_46__typed_closure
    struct.new $user__closure_i64_T6_Int_Int
    local.set $p1
    i64.const 0
    local.get $p1
    struct.new $user__iter_state__Int__Int
    local.set $p2
    local.get $p2
    struct.get $user__iter_state__Int__Int 0
    struct.new $rt_types__BoxedInt
    local.get $p2
    struct.get $user__iter_state__Int__Int 1
    struct.new $rt_types__IterState
    return
  )
  (func $user__func_43 (type $functype_28)
    (param $p0 (ref null $rt_types__String))
    (param $p1 i64)
    (param $p2 i64)
    (result (ref null $rt_types__String))
    (local $p3 (ref null $rt_types__String))
    (local $p4 (ref $rt_types__String))
    (local $p5 (ref $rt_types__String))
    (local $p6 (ref $rt_types__String))
    (local $p7 (ref $rt_types__String))
    (local $p8 (ref $rt_types__String))
    (local $p9 (ref $rt_types__String))
    (local $p10 (ref $rt_types__String))
    local.get $p0
    local.set $p3
    local.get $p1
    call $rt_str__from_i64
    local.set $p4
    local.get $p2
    call $rt_str__from_i64
    local.set $p5
    call $user____str_lit_get_3d
    local.get $p5
    call $rt_str__concat
    local.set $p6
    local.get $p4
    local.get $p6
    call $rt_str__concat
    local.set $p7
    call $user____str_lit_get_5f
    local.get $p7
    call $rt_str__concat
    local.set $p8
    local.get $p3
    local.get $p8
    call $rt_str__concat
    local.set $p9
    call $user____str_lit_get_empty
    local.get $p9
    call $rt_str__concat
    local.set $p10
    local.get $p10
    return
  )
  (func $user__func_44 (type $functype_29)
    (param $p0 (ref null $rt_types__Array))
    (result (ref null $rt_types__IterState))
    (local $p1 i64)
    (local $p2 i64)
    (local $p3 (ref null $user__closure_i64_T6_String_Int))
    (local $p4 (ref null $user__iter_state__String__Int))
    local.get $p0
    call $rt_arr__len
    i64.extend_i32_s
    local.set $p1
    local.get $p1
    local.set $p2
    local.get $p2
    struct.new $rt_types__BoxedInt
    global.set $user__global_local_1
    ref.func $user__func_47__closure
    local.get $p0
    local.get $p2
    struct.new $rt_types__BoxedInt
    array.new_fixed $rt_types__ClosureEnv 2
    ref.func $user__func_47__typed_closure
    struct.new $user__closure_i64_T6_String_Int
    local.set $p3
    i64.const 0
    local.get $p3
    struct.new $user__iter_state__String__Int
    local.set $p4
    local.get $p4
    struct.get $user__iter_state__String__Int 0
    struct.new $rt_types__BoxedInt
    local.get $p4
    struct.get $user__iter_state__String__Int 1
    struct.new $rt_types__IterState
    return
  )
  (func $user__func_48 (type $functype_30)
    (local $p0 i32)
    (local $p1 (ref null $rt_types__IterState))
    (local $p2 (ref null $rt_types__IterState))
    (local $p3 (ref null $rt_types__Variant))
    (local $p4 (ref null $rt_types__Variant))
    (local $p5 (ref null $user__UserRecord_5))
    (local $p6 (ref null $rt_types__String))
    (local $p7 (ref null $rt_types__String))
    (local $p8 (ref null $rt_types__IterState))
    (local $p9 i32)
    (local $p10 (ref null $rt_types__String))
    (local $p11 (ref $rt_types__String))
    (local $p12 i32)
    (local $p13 i32)
    (local $p14 i32)
    (local $p15 i32)
    (local $p16 i32)
    (local $p17 (ref null $rt_types__IterState))
    (local $p18 anyref)
    (local $p19 (ref null $rt_types__Variant))
    (local $p20 (ref null $rt_types__Variant))
    (local $p21 (ref null $user__UserRecord_5))
    (local $p22 i64)
    (local $p23 i64)
    (local $p24 (ref null $rt_types__IterState))
    (local $p25 i32)
    (local $p26 (ref $rt_types__String))
    (local $p27 (ref $rt_types__String))
    (local $p28 i32)
    (local $p29 i32)
    (local $p30 i32)
    (local $p31 i32)
    (local $p32 i32)
    (local $p33 (ref null $rt_types__Array))
    (local $p34 (ref null $rt_types__IterState))
    (local $p35 anyref)
    (local $p36 (ref null $rt_types__Variant))
    (local $p37 (ref null $rt_types__Variant))
    (local $p38 (ref null $user__UserRecord_5))
    (local $p39 (ref null $rt_types__String))
    (local $p40 (ref null $rt_types__String))
    (local $p41 (ref null $rt_types__IterState))
    (local $p42 i32)
    (local $p43 (ref null $rt_types__String))
    (local $p44 (ref $rt_types__String))
    (local $p45 i32)
    (local $p46 i32)
    (local $p47 i32)
    (local $p48 i32)
    call $user____str_lit_get_63686172733a
    call $rt_core__print
    i32.const 0
    local.set $p0
    call $user____str_lit_get_48656c6c6f
    call $user__func_41
    local.set $p1
    local.get $p1
    local.set $p2
    local.get $p2
    global.set $user__global_local_0
    (block $break_0 (result i32)
      (loop $cont_0
        local.get $p2
        call $user__user____iterator_next
        local.set $p3
        local.get $p3
        local.set $p4
        local.get $p4
        global.set $user__global_local_1
        local.get $p4
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 0
        i32.const 0
        i32.eq
        local.get $p4
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 1
        i32.const 0
        i32.eq
        i32.and
        (if
          (then
            i32.const 0
            br $break_0)
          (else
            local.get $p4
            ref.cast (ref null $rt_types__Variant)
            struct.get $rt_types__Variant 0
            i32.const 0
            i32.eq
            local.get $p4
            ref.cast (ref null $rt_types__Variant)
            struct.get $rt_types__Variant 1
            i32.const 1
            i32.eq
            i32.and
            (if
              (then
                local.get $p4
                ref.cast (ref null $rt_types__Variant)
                struct.get $rt_types__Variant 2
                i32.const 0
                array.get $rt_types__Array
                ref.cast (ref null $user__UserRecord_5)
                local.set $p5
                local.get $p5
                struct.get $user__UserRecord_5 0
                ref.cast (ref null $rt_types__String)
                local.set $p6
                local.get $p6
                local.set $p7
                local.get $p5
                struct.get $user__UserRecord_5 1
                local.set $p8
                local.get $p8
                local.set $p2
                local.get $p2
                global.set $user__global_local_0
                i32.const 0
                local.set $p9
                local.get $p7
                local.set $p10
                call $user____str_lit_get_20
                local.get $p10
                call $rt_str__concat
                local.set $p11
                local.get $p11
                call $rt_core__print
                i32.const 0
                local.set $p12
                br $cont_0)
              (else
                call $user____str_lit_get_6e6f6e2d65786861757374697665206d61746368
                call $rt_core__trap
                unreachable
                unreachable))
            unreachable))
        local.get $p13
        drop
        br $cont_0)
      unreachable)
    local.set $p14
    call $user____str_lit_get_empty
    call $rt_core__println
    i32.const 0
    local.set $p15
    call $user____str_lit_get_63756d756c3a
    call $rt_core__print
    i32.const 0
    local.set $p16
    i64.const 5
    call $user__func_42
    local.set $p17
    local.get $p17
    local.set $p18
    (block $break_1 (result i32)
      (loop $cont_1
        local.get $p18
        ref.cast (ref null $rt_types__IterState)
        call $user__user____iterator_next
        local.set $p19
        local.get $p19
        local.set $p20
        local.get $p20
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 0
        i32.const 0
        i32.eq
        local.get $p20
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 1
        i32.const 0
        i32.eq
        i32.and
        (if
          (then
            i32.const 0
            br $break_1)
          (else
            local.get $p20
            ref.cast (ref null $rt_types__Variant)
            struct.get $rt_types__Variant 0
            i32.const 0
            i32.eq
            local.get $p20
            ref.cast (ref null $rt_types__Variant)
            struct.get $rt_types__Variant 1
            i32.const 1
            i32.eq
            i32.and
            (if
              (then
                local.get $p20
                ref.cast (ref null $rt_types__Variant)
                struct.get $rt_types__Variant 2
                i32.const 0
                array.get $rt_types__Array
                ref.cast (ref null $user__UserRecord_5)
                local.set $p21
                local.get $p21
                struct.get $user__UserRecord_5 0
                ref.cast (ref $rt_types__BoxedInt)
                struct.get $rt_types__BoxedInt 0
                local.set $p22
                local.get $p22
                local.set $p23
                local.get $p21
                struct.get $user__UserRecord_5 1
                local.set $p24
                local.get $p24
                local.set $p18
                i32.const 0
                local.set $p25
                local.get $p23
                call $rt_str__from_i64
                local.set $p26
                call $user____str_lit_get_20
                local.get $p26
                call $rt_str__concat
                local.set $p27
                local.get $p27
                call $rt_core__print
                i32.const 0
                local.set $p28
                br $cont_1)
              (else
                call $user____str_lit_get_6e6f6e2d65786861757374697665206d61746368
                call $rt_core__trap
                unreachable
                unreachable))
            unreachable))
        local.get $p29
        drop
        br $cont_1)
      unreachable)
    local.set $p30
    call $user____str_lit_get_empty
    call $rt_core__println
    i32.const 0
    local.set $p31
    call $user____str_lit_get_6c6162656c733a
    call $rt_core__print
    i32.const 0
    local.set $p32
    i64.const 10
    struct.new $rt_types__BoxedInt
    i64.const 20
    struct.new $rt_types__BoxedInt
    i64.const 30
    struct.new $rt_types__BoxedInt
    array.new_fixed $rt_types__Array 3
    local.set $p33
    local.get $p33
    call $user__func_44
    local.set $p34
    local.get $p34
    local.set $p35
    (block $break_2 (result i32)
      (loop $cont_2
        local.get $p35
        ref.cast (ref null $rt_types__IterState)
        call $user__user____iterator_next
        local.set $p36
        local.get $p36
        local.set $p37
        local.get $p37
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 0
        i32.const 0
        i32.eq
        local.get $p37
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 1
        i32.const 0
        i32.eq
        i32.and
        (if
          (then
            i32.const 0
            br $break_2)
          (else
            local.get $p37
            ref.cast (ref null $rt_types__Variant)
            struct.get $rt_types__Variant 0
            i32.const 0
            i32.eq
            local.get $p37
            ref.cast (ref null $rt_types__Variant)
            struct.get $rt_types__Variant 1
            i32.const 1
            i32.eq
            i32.and
            (if
              (then
                local.get $p37
                ref.cast (ref null $rt_types__Variant)
                struct.get $rt_types__Variant 2
                i32.const 0
                array.get $rt_types__Array
                ref.cast (ref null $user__UserRecord_5)
                local.set $p38
                local.get $p38
                struct.get $user__UserRecord_5 0
                ref.cast (ref null $rt_types__String)
                local.set $p39
                local.get $p39
                local.set $p40
                local.get $p38
                struct.get $user__UserRecord_5 1
                local.set $p41
                local.get $p41
                local.set $p35
                i32.const 0
                local.set $p42
                local.get $p40
                local.set $p43
                call $user____str_lit_get_20
                local.get $p43
                call $rt_str__concat
                local.set $p44
                local.get $p44
                call $rt_core__print
                i32.const 0
                local.set $p45
                br $cont_2)
              (else
                call $user____str_lit_get_6e6f6e2d65786861757374697665206d61746368
                call $rt_core__trap
                unreachable
                unreachable))
            unreachable))
        local.get $p46
        drop
        br $cont_2)
      unreachable)
    local.set $p47
    call $user____str_lit_get_empty
    return_call $rt_core__println
  )
  (func $user__func_45 (type $functype_31)
    (param $p0 i64)
    (param $p1 anyref)
    (result (ref null $rt_types__Variant))
    (local $p2 i64)
    (local $p3 i32)
    (local $p4 (ref null $user__unfold_step__String__Int))
    (local $p5 i32)
    (local $p6 i64)
    (local $p7 i64)
    (local $p8 i32)
    (local $p9 i32)
    (local $p10 i32)
    (local $p11 i64)
    (local $p12 i64)
    (local $p13 i64)
    (local $p14 i64)
    (local $p15 i64)
    (local $p16 (ref null $rt_types__String))
    (local $p17 i64)
    (local $p18 (ref null $user__unfold_step__String__Int))
    (local $p19 (ref null $user__unfold_step__String__Int))
    local.get $p1
    ref.cast (ref null $rt_types__String)
    call $rt_str__len
    i64.extend_i32_s
    local.set $p2
    local.get $p0
    local.get $p2
    i64.ge_s
    local.set $p3
    local.get $p3
    (if (result (ref null $user__unfold_step__String__Int))
      (then
        i32.const 0
        ref.null none
        i64.const 0
        struct.new $user__unfold_step__String__Int
        local.set $p4
        local.get $p4)
      (else
        local.get $p0
        i64.const 0
        i64.ge_s
        local.get $p0
        local.get $p1
        ref.cast (ref null $rt_types__String)
        array.len
        i64.extend_i32_u
        i64.lt_s
        i32.and
        (if
          (then)
          (else
            unreachable))
        local.get $p1
        ref.cast (ref null $rt_types__String)
        ref.as_non_null
        local.get $p0
        i32.wrap_i64
        array.get_u $rt_types__String
        local.set $p5
        local.get $p5
        i64.extend_i32_u
        local.set $p6
        local.get $p6
        local.set $p7
        local.get $p7
        i64.const 128
        i64.lt_s
        local.set $p8
        local.get $p8
        (if (result i64)
          (then
            i64.const 1)
          (else
            local.get $p7
            i64.const 224
            i64.lt_s
            local.set $p9
            local.get $p9
            (if (result i64)
              (then
                i64.const 2)
              (else
                local.get $p7
                i64.const 240
                i64.lt_s
                local.set $p10
                local.get $p10
                (if (result i64)
                  (then
                    i64.const 3)
                  (else
                    i64.const 4))
                local.set $p11
                local.get $p11))
            local.set $p12
            local.get $p12))
        local.set $p13
        local.get $p13
        local.set $p14
        local.get $p0
        local.get $p14
        i64.add
        local.set $p15
        local.get $p0
        i64.const 0
        i64.ge_s
        local.get $p15
        i64.const 0
        i64.ge_s
        i32.and
        local.get $p0
        local.get $p15
        i64.le_s
        i32.and
        local.get $p15
        local.get $p1
        ref.cast (ref null $rt_types__String)
        array.len
        i64.extend_i32_u
        i64.le_s
        i32.and
        (if
          (then)
          (else
            unreachable))
        local.get $p0
        i32.wrap_i64
        i32.const 0
        i32.gt_u
        local.get $p0
        i32.wrap_i64
        local.get $p1
        ref.cast (ref null $rt_types__String)
        array.len
        i32.lt_u
        i32.and
        (if
          (then
            local.get $p1
            ref.cast (ref null $rt_types__String)
            ref.as_non_null
            local.get $p0
            i32.wrap_i64
            array.get_u $rt_types__String
            i32.const 192
            i32.and
            i32.const 128
            i32.eq
            (if
              (then
                unreachable))))
        local.get $p15
        i32.wrap_i64
        local.get $p1
        ref.cast (ref null $rt_types__String)
        array.len
        i32.lt_u
        (if
          (then
            local.get $p1
            ref.cast (ref null $rt_types__String)
            ref.as_non_null
            local.get $p15
            i32.wrap_i64
            array.get_u $rt_types__String
            i32.const 192
            i32.and
            i32.const 128
            i32.eq
            (if
              (then
                unreachable))))
        local.get $p1
        ref.cast (ref null $rt_types__String)
        local.get $p0
        i32.wrap_i64
        local.get $p15
        i32.wrap_i64
        call $rt_str__substring
        local.set $p16
        local.get $p0
        local.get $p14
        i64.add
        local.set $p17
        i32.const 1
        local.get $p16
        local.get $p17
        struct.new $user__unfold_step__String__Int
        local.set $p18
        local.get $p18))
    local.set $p19
    local.get $p19
    struct.get $user__unfold_step__String__Int 0
    i32.eqz
    (if (result (ref null $rt_types__Variant))
      (then
        i32.const 6
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        i32.const 6
        i32.const 1
        local.get $p19
        struct.get $user__unfold_step__String__Int 1
        local.get $p19
        struct.get $user__unfold_step__String__Int 2
        struct.new $rt_types__BoxedInt
        array.new_fixed $rt_types__Array 2
        struct.new $rt_types__Variant))
    return
  )
  (func $user__func_46 (type $functype_31)
    (param $p0 i64)
    (param $p1 anyref)
    (result (ref null $rt_types__Variant))
    (local $p2 i64)
    (local $p3 i64)
    (local $p4 i64)
    (local $p5 i64)
    (local $p6 i32)
    (local $p7 (ref null $user__unfold_step__Int__Int))
    (local $p8 i64)
    (local $p9 i64)
    (local $p10 i64)
    (local $p11 i64)
    (local $p12 i64)
    (local $p13 i64)
    (local $p14 i64)
    (local $p15 (ref null $user__unfold_step__Int__Int))
    (local $p16 (ref null $user__unfold_step__Int__Int))
    local.get $p0
    i64.const 10000
    i64.div_s
    local.set $p2
    local.get $p2
    local.set $p3
    local.get $p0
    i64.const 10000
    i64.rem_s
    local.set $p4
    local.get $p4
    local.set $p5
    local.get $p3
    local.get $p1
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    i64.ge_s
    local.set $p6
    local.get $p6
    (if (result (ref null $user__unfold_step__Int__Int))
      (then
        i32.const 0
        i64.const 0
        i64.const 0
        struct.new $user__unfold_step__Int__Int
        local.set $p7
        local.get $p7)
      (else
        local.get $p3
        i64.const 1
        i64.add
        local.set $p8
        local.get $p8
        local.set $p9
        local.get $p5
        local.get $p9
        i64.add
        local.set $p10
        local.get $p10
        local.set $p11
        local.get $p3
        i64.const 1
        i64.add
        local.set $p12
        local.get $p12
        i64.const 10000
        i64.mul
        local.set $p13
        local.get $p13
        local.get $p11
        i64.add
        local.set $p14
        i32.const 1
        local.get $p11
        local.get $p14
        struct.new $user__unfold_step__Int__Int
        local.set $p15
        local.get $p15))
    local.set $p16
    local.get $p16
    struct.get $user__unfold_step__Int__Int 0
    i32.eqz
    (if (result (ref null $rt_types__Variant))
      (then
        i32.const 6
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        i32.const 6
        i32.const 1
        local.get $p16
        struct.get $user__unfold_step__Int__Int 1
        struct.new $rt_types__BoxedInt
        local.get $p16
        struct.get $user__unfold_step__Int__Int 2
        struct.new $rt_types__BoxedInt
        array.new_fixed $rt_types__Array 2
        struct.new $rt_types__Variant))
    return
  )
  (func $user__func_47 (type $functype_32)
    (param $p0 i64)
    (param $p1 anyref)
    (param $p2 anyref)
    (result (ref null $rt_types__Variant))
    (local $p3 i32)
    (local $p4 (ref null $user__unfold_step__String__Int))
    (local $p5 i64)
    (local $p6 (ref null $rt_types__String))
    (local $p7 (ref null $rt_types__String))
    (local $p8 i64)
    (local $p9 (ref null $user__unfold_step__String__Int))
    (local $p10 (ref null $user__unfold_step__String__Int))
    local.get $p0
    local.get $p2
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    i64.ge_s
    local.set $p3
    local.get $p3
    (if (result (ref null $user__unfold_step__String__Int))
      (then
        i32.const 0
        ref.null none
        i64.const 0
        struct.new $user__unfold_step__String__Int
        local.set $p4
        local.get $p4)
      (else
        local.get $p1
        ref.cast (ref null $rt_types__Array)
        local.get $p0
        i32.wrap_i64
        call $rt_arr__get
        ref.cast (ref $rt_types__BoxedInt)
        struct.get $rt_types__BoxedInt 0
        local.set $p5
        call $user____str_lit_get_6974656d
        local.get $p0
        local.get $p5
        call $user__func_43
        local.set $p6
        local.get $p6
        local.set $p7
        local.get $p0
        i64.const 1
        i64.add
        local.set $p8
        i32.const 1
        local.get $p7
        local.get $p8
        struct.new $user__unfold_step__String__Int
        local.set $p9
        local.get $p9))
    local.set $p10
    local.get $p10
    struct.get $user__unfold_step__String__Int 0
    i32.eqz
    (if (result (ref null $rt_types__Variant))
      (then
        i32.const 6
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        i32.const 6
        i32.const 1
        local.get $p10
        struct.get $user__unfold_step__String__Int 1
        local.get $p10
        struct.get $user__unfold_step__String__Int 2
        struct.new $rt_types__BoxedInt
        array.new_fixed $rt_types__Array 2
        struct.new $rt_types__Variant))
    return
  )
  (func $user__func_41__closure (type $functype_33)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__String)
    call $user__func_41
  )
  (func $user__func_42__closure (type $functype_33)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    call $user__func_42
  )
  (func $user__func_43__closure (type $functype_33)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__String)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 1
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 2
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    call $user__func_43
  )
  (func $user__func_44__closure (type $functype_33)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__Array)
    call $user__func_44
  )
  (func $user__func_48__closure (type $functype_33)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    call $user__func_48
    i32.const 0
    ref.i31
  )
  (func $user__func_45__closure (type $functype_33)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    local.get $p0
    ref.cast (ref null $rt_types__ClosureEnv)
    i32.const 0
    array.get $rt_types__ClosureEnv
    call $user__func_45
  )
  (func $user__func_45__typed_closure (type $functype_34)
    (param $p0 (ref null $rt_types__ClosureEnv))
    (param $p1 i64)
    (result (ref null $rt_types__Variant))
    local.get $p1
    local.get $p0
    ref.cast (ref null $rt_types__ClosureEnv)
    i32.const 0
    array.get $rt_types__ClosureEnv
    call $user__func_45
  )
  (func $user__func_46__closure (type $functype_33)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    local.get $p0
    ref.cast (ref null $rt_types__ClosureEnv)
    i32.const 0
    array.get $rt_types__ClosureEnv
    call $user__func_46
  )
  (func $user__func_46__typed_closure (type $functype_34)
    (param $p0 (ref null $rt_types__ClosureEnv))
    (param $p1 i64)
    (result (ref null $rt_types__Variant))
    local.get $p1
    local.get $p0
    ref.cast (ref null $rt_types__ClosureEnv)
    i32.const 0
    array.get $rt_types__ClosureEnv
    call $user__func_46
  )
  (func $user__func_47__closure (type $functype_33)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    local.get $p0
    ref.cast (ref null $rt_types__ClosureEnv)
    i32.const 0
    array.get $rt_types__ClosureEnv
    local.get $p0
    ref.cast (ref null $rt_types__ClosureEnv)
    i32.const 1
    array.get $rt_types__ClosureEnv
    call $user__func_47
  )
  (func $user__func_47__typed_closure (type $functype_34)
    (param $p0 (ref null $rt_types__ClosureEnv))
    (param $p1 i64)
    (result (ref null $rt_types__Variant))
    local.get $p1
    local.get $p0
    ref.cast (ref null $rt_types__ClosureEnv)
    i32.const 0
    array.get $rt_types__ClosureEnv
    local.get $p0
    ref.cast (ref null $rt_types__ClosureEnv)
    i32.const 1
    array.get $rt_types__ClosureEnv
    call $user__func_47
  )
  (func $user__user____iterator_next (type $functype_35)
    (param $p0 anyref)
    (result (ref null $rt_types__Variant))
    (local $p1 (ref null $rt_types__Variant))
    (local $p2 i32)
    (local $p3 anyref)
    (local $p4 (ref null $rt_types__IterState))
    local.get $p0
    ref.cast (ref null $rt_types__IterState)
    local.set $p4
    local.get $p4
    struct.get $rt_types__IterState 1
    ref.cast (ref $rt_types__Closure)
    struct.get $rt_types__Closure 1
    local.get $p4
    struct.get $rt_types__IterState 0
    array.new_fixed $rt_types__Array 1
    local.get $p4
    struct.get $rt_types__IterState 1
    ref.cast (ref $rt_types__Closure)
    struct.get $rt_types__Closure 0
    call_ref $rt_types__ClosureFunc
    ref.cast (ref $rt_types__Variant)
    local.set $p1
    local.get $p1
    struct.get $rt_types__Variant 1
    local.set $p2
    local.get $p2
    i32.eqz
    (if (result (ref null $rt_types__Variant))
      (then
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        local.get $p1
        struct.get $rt_types__Variant 2
        local.set $p3
        local.get $p3
        ref.cast (ref null $rt_types__Array)
        i32.const 0
        array.get $rt_types__Array
        local.get $p3
        ref.cast (ref null $rt_types__Array)
        i32.const 1
        array.get $rt_types__Array
        local.get $p4
        struct.get $rt_types__IterState 1
        struct.new $rt_types__IterState
        struct.new $user__UserRecord_5
        local.set $p3
        i32.const 0
        i32.const 1
        local.get $p3
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant))
    return
  )
  (func $user__$int_from_string_helper (type $functype_36)
    (param $p0 (ref null $rt_types__String))
    (result anyref)
    (local $p1 i64)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i64)
    (local $p5 i32)
    (local $p6 i32)
    i64.const 1
    local.set $p4
    i32.const 1
    local.set $p6
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p3
    i32.eqz
    (if
      (then
        i32.const 0
        local.set $p6)
      (else
        local.get $p0
        ref.as_non_null
        i32.const 0
        array.get_u $rt_types__String
        local.set $p5
        local.get $p5
        i32.const 45
        i32.eq
        (if
          (then
            i64.const -1
            local.set $p4
            i32.const 1
            local.set $p2
            local.get $p3
            i32.const 1
            i32.eq
            (if
              (then
                i32.const 0
                local.set $p6)))
          (else
            local.get $p5
            i32.const 43
            i32.eq
            (if
              (then
                i32.const 1
                local.set $p2
                local.get $p3
                i32.const 1
                i32.eq
                (if
                  (then
                    i32.const 0
                    local.set $p6)))
              (else
                i32.const 0
                local.set $p2))))
        local.get $p6
        (if
          (then
            (block $$done
              (loop $$digit_loop
                local.get $p2
                local.get $p3
                i32.ge_s
                br_if $$done
                local.get $p0
                ref.as_non_null
                local.get $p2
                array.get_u $rt_types__String
                local.set $p5
                local.get $p5
                i32.const 48
                i32.lt_s
                local.get $p5
                i32.const 57
                i32.gt_s
                i32.or
                (if
                  (then
                    i32.const 0
                    local.set $p6
                    br $$done))
                local.get $p1
                i64.const 10
                i64.mul
                local.get $p5
                i32.const 48
                i32.sub
                i64.extend_i32_u
                i64.add
                local.set $p1
                local.get $p2
                i32.const 1
                i32.add
                local.set $p2
                br $$digit_loop))))))
    local.get $p6
    (if (result anyref)
      (then
        i32.const 0
        i32.const 1
        local.get $p1
        local.get $p4
        i64.mul
        struct.new $rt_types__BoxedInt
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant)
      (else
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant))
  )
  (func $user__$from_code_point_helper (type $functype_37)
    (param $p0 i32)
    (result anyref)
    local.get $p0
    i32.const 0
    i32.lt_s
    (if (result anyref)
      (then
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        local.get $p0
        i32.const 128
        i32.lt_u
        (if (result anyref)
          (then
            i32.const 0
            i32.const 1
            local.get $p0
            array.new_fixed $rt_types__String 1
            array.new_fixed $rt_types__Array 1
            struct.new $rt_types__Variant)
          (else
            local.get $p0
            i32.const 2048
            i32.lt_u
            (if (result anyref)
              (then
                i32.const 0
                i32.const 1
                local.get $p0
                i32.const 6
                i32.shr_u
                i32.const 192
                i32.or
                local.get $p0
                i32.const 63
                i32.and
                i32.const 128
                i32.or
                array.new_fixed $rt_types__String 2
                array.new_fixed $rt_types__Array 1
                struct.new $rt_types__Variant)
              (else
                local.get $p0
                i32.const 55296
                i32.ge_u
                local.get $p0
                i32.const 57343
                i32.le_u
                i32.and
                (if (result anyref)
                  (then
                    i32.const 0
                    i32.const 0
                    array.new_fixed $rt_types__Array 0
                    struct.new $rt_types__Variant)
                  (else
                    local.get $p0
                    i32.const 65535
                    i32.le_u
                    (if (result anyref)
                      (then
                        i32.const 0
                        i32.const 1
                        local.get $p0
                        i32.const 12
                        i32.shr_u
                        i32.const 224
                        i32.or
                        local.get $p0
                        i32.const 6
                        i32.shr_u
                        i32.const 63
                        i32.and
                        i32.const 128
                        i32.or
                        local.get $p0
                        i32.const 63
                        i32.and
                        i32.const 128
                        i32.or
                        array.new_fixed $rt_types__String 3
                        array.new_fixed $rt_types__Array 1
                        struct.new $rt_types__Variant)
                      (else
                        local.get $p0
                        i32.const 1114111
                        i32.le_u
                        (if (result anyref)
                          (then
                            i32.const 0
                            i32.const 1
                            local.get $p0
                            i32.const 18
                            i32.shr_u
                            i32.const 240
                            i32.or
                            local.get $p0
                            i32.const 12
                            i32.shr_u
                            i32.const 63
                            i32.and
                            i32.const 128
                            i32.or
                            local.get $p0
                            i32.const 6
                            i32.shr_u
                            i32.const 63
                            i32.and
                            i32.const 128
                            i32.or
                            local.get $p0
                            i32.const 63
                            i32.and
                            i32.const 128
                            i32.or
                            array.new_fixed $rt_types__String 4
                            array.new_fixed $rt_types__Array 1
                            struct.new $rt_types__Variant)
                          (else
                            i32.const 0
                            i32.const 0
                            array.new_fixed $rt_types__Array 0
                            struct.new $rt_types__Variant))))))))))))
  )
  (func $user__$string_utf8_bytes_helper (type $functype_38)
    (param $p0 (ref null $rt_types__String))
    (result (ref $rt_types__Array))
    (local $p1 i32)
    (local $p2 i32)
    (local $p3 (ref $rt_types__Array))
    local.get $p0
    array.len
    local.set $p1
    local.get $p1
    array.new_default $rt_types__Array
    local.set $p3
    i32.const 0
    local.set $p2
    (block $$break
      (loop $$continue
        local.get $p2
        local.get $p1
        i32.ge_u
        br_if $$break
        local.get $p3
        local.get $p2
        local.get $p0
        local.get $p2
        array.get_u $rt_types__String
        ref.i31
        array.set $rt_types__Array
        local.get $p2
        i32.const 1
        i32.add
        local.set $p2
        br $$continue))
    local.get $p3
  )
  (func $user__$string_from_utf8_helper (type $functype_39)
    (param $p0 (ref null $rt_types__Array))
    (result anyref)
    (local $p1 i32)
    (local $p2 i32)
    (local $p3 (ref null $rt_types__String))
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 i32)
    local.get $p0
    array.len
    local.set $p1
    i32.const 1
    local.set $p5
    i32.const 0
    local.set $p2
    (block $$vbreak
      (loop $$vcont
        local.get $p2
        local.get $p1
        i32.ge_u
        br_if $$vbreak
        local.get $p0
        local.get $p2
        array.get $rt_types__Array
        ref.cast (ref i31)
        i31.get_u
        local.set $p4
        local.get $p4
        i32.const 128
        i32.lt_u
        (if
          (then
            local.get $p2
            i32.const 1
            i32.add
            local.set $p2)
          (else
            local.get $p4
            i32.const 192
            i32.ge_u
            local.get $p4
            i32.const 223
            i32.le_u
            i32.and
            (if
              (then
                local.get $p4
                i32.const 194
                i32.lt_u
                (if
                  (then
                    i32.const 0
                    local.set $p5
                    br $$vbreak))
                local.get $p2
                i32.const 1
                i32.add
                local.get $p1
                i32.ge_u
                (if
                  (then
                    i32.const 0
                    local.set $p5
                    br $$vbreak))
                local.get $p0
                local.get $p2
                i32.const 1
                i32.add
                array.get $rt_types__Array
                ref.cast (ref i31)
                i31.get_u
                local.set $p6
                local.get $p6
                i32.const 192
                i32.and
                i32.const 128
                i32.ne
                (if
                  (then
                    i32.const 0
                    local.set $p5
                    br $$vbreak))
                local.get $p2
                i32.const 2
                i32.add
                local.set $p2)
              (else
                local.get $p4
                i32.const 224
                i32.ge_u
                local.get $p4
                i32.const 239
                i32.le_u
                i32.and
                (if
                  (then
                    local.get $p2
                    i32.const 2
                    i32.add
                    local.get $p1
                    i32.ge_u
                    (if
                      (then
                        i32.const 0
                        local.set $p5
                        br $$vbreak))
                    local.get $p0
                    local.get $p2
                    i32.const 1
                    i32.add
                    array.get $rt_types__Array
                    ref.cast (ref i31)
                    i31.get_u
                    i32.const 192
                    i32.and
                    i32.const 128
                    i32.ne
                    (if
                      (then
                        i32.const 0
                        local.set $p5
                        br $$vbreak))
                    local.get $p0
                    local.get $p2
                    i32.const 2
                    i32.add
                    array.get $rt_types__Array
                    ref.cast (ref i31)
                    i31.get_u
                    i32.const 192
                    i32.and
                    i32.const 128
                    i32.ne
                    (if
                      (then
                        i32.const 0
                        local.set $p5
                        br $$vbreak))
                    local.get $p4
                    i32.const 224
                    i32.eq
                    (if
                      (then
                        local.get $p0
                        local.get $p2
                        i32.const 1
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 160
                        i32.lt_u
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))))
                    local.get $p4
                    i32.const 237
                    i32.eq
                    (if
                      (then
                        local.get $p0
                        local.get $p2
                        i32.const 1
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 160
                        i32.ge_u
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))))
                    local.get $p2
                    i32.const 3
                    i32.add
                    local.set $p2)
                  (else
                    local.get $p4
                    i32.const 240
                    i32.ge_u
                    local.get $p4
                    i32.const 244
                    i32.le_u
                    i32.and
                    (if
                      (then
                        local.get $p2
                        i32.const 3
                        i32.add
                        local.get $p1
                        i32.ge_u
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p0
                        local.get $p2
                        i32.const 1
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 192
                        i32.and
                        i32.const 128
                        i32.ne
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p0
                        local.get $p2
                        i32.const 2
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 192
                        i32.and
                        i32.const 128
                        i32.ne
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p0
                        local.get $p2
                        i32.const 3
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 192
                        i32.and
                        i32.const 128
                        i32.ne
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p4
                        i32.const 240
                        i32.eq
                        (if
                          (then
                            local.get $p0
                            local.get $p2
                            i32.const 1
                            i32.add
                            array.get $rt_types__Array
                            ref.cast (ref i31)
                            i31.get_u
                            i32.const 144
                            i32.lt_u
                            (if
                              (then
                                i32.const 0
                                local.set $p5
                                br $$vbreak))))
                        local.get $p4
                        i32.const 244
                        i32.eq
                        (if
                          (then
                            local.get $p0
                            local.get $p2
                            i32.const 1
                            i32.add
                            array.get $rt_types__Array
                            ref.cast (ref i31)
                            i31.get_u
                            i32.const 144
                            i32.ge_u
                            (if
                              (then
                                i32.const 0
                                local.set $p5
                                br $$vbreak))))
                        local.get $p2
                        i32.const 4
                        i32.add
                        local.set $p2)
                      (else
                        i32.const 0
                        local.set $p5
                        br $$vbreak))))))))
        br $$vcont))
    local.get $p5
    i32.eqz
    (if (result anyref)
      (then
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        local.get $p1
        array.new_default $rt_types__String
        local.set $p3
        i32.const 0
        local.set $p2
        (block $$cbreak
          (loop $$ccont
            local.get $p2
            local.get $p1
            i32.ge_u
            br_if $$cbreak
            local.get $p3
            local.get $p2
            local.get $p0
            local.get $p2
            array.get $rt_types__Array
            ref.cast (ref i31)
            i31.get_u
            array.set $rt_types__String
            local.get $p2
            i32.const 1
            i32.add
            local.set $p2
            br $$ccont))
        i32.const 0
        i32.const 1
        local.get $p3
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant))
  )
  (func $user____user_init (type $functype_30)
    call $user__func_48
  )
  (func $user____str_lit_get_empty (type $functype_40)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_empty
    ref.is_null
    (if
      (then
        array.new_fixed $rt_types__String 0
        global.set $user____str_lit_global_empty))
    global.get $user____str_lit_global_empty
    ref.as_non_null
  )
  (func $user____str_lit_get_20 (type $functype_40)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_20
    ref.is_null
    (if
      (then
        i32.const 32
        array.new_fixed $rt_types__String 1
        global.set $user____str_lit_global_20))
    global.get $user____str_lit_global_20
    ref.as_non_null
  )
  (func $user____str_lit_get_3d (type $functype_40)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_3d
    ref.is_null
    (if
      (then
        i32.const 61
        array.new_fixed $rt_types__String 1
        global.set $user____str_lit_global_3d))
    global.get $user____str_lit_global_3d
    ref.as_non_null
  )
  (func $user____str_lit_get_48656c6c6f (type $functype_40)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_48656c6c6f
    ref.is_null
    (if
      (then
        i32.const 72
        i32.const 101
        i32.const 108
        i32.const 108
        i32.const 111
        array.new_fixed $rt_types__String 5
        global.set $user____str_lit_global_48656c6c6f))
    global.get $user____str_lit_global_48656c6c6f
    ref.as_non_null
  )
  (func $user____str_lit_get_5f (type $functype_40)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_5f
    ref.is_null
    (if
      (then
        i32.const 95
        array.new_fixed $rt_types__String 1
        global.set $user____str_lit_global_5f))
    global.get $user____str_lit_global_5f
    ref.as_non_null
  )
  (func $user____str_lit_get_63686172733a (type $functype_40)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_63686172733a
    ref.is_null
    (if
      (then
        i32.const 99
        i32.const 104
        i32.const 97
        i32.const 114
        i32.const 115
        i32.const 58
        array.new_fixed $rt_types__String 6
        global.set $user____str_lit_global_63686172733a))
    global.get $user____str_lit_global_63686172733a
    ref.as_non_null
  )
  (func $user____str_lit_get_63756d756c3a (type $functype_40)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_63756d756c3a
    ref.is_null
    (if
      (then
        i32.const 99
        i32.const 117
        i32.const 109
        i32.const 117
        i32.const 108
        i32.const 58
        array.new_fixed $rt_types__String 6
        global.set $user____str_lit_global_63756d756c3a))
    global.get $user____str_lit_global_63756d756c3a
    ref.as_non_null
  )
  (func $user____str_lit_get_6974656d (type $functype_40)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_6974656d
    ref.is_null
    (if
      (then
        i32.const 105
        i32.const 116
        i32.const 101
        i32.const 109
        array.new_fixed $rt_types__String 4
        global.set $user____str_lit_global_6974656d))
    global.get $user____str_lit_global_6974656d
    ref.as_non_null
  )
  (func $user____str_lit_get_6c6162656c733a (type $functype_40)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_6c6162656c733a
    ref.is_null
    (if
      (then
        i32.const 108
        i32.const 97
        i32.const 98
        i32.const 101
        i32.const 108
        i32.const 115
        i32.const 58
        array.new_fixed $rt_types__String 7
        global.set $user____str_lit_global_6c6162656c733a))
    global.get $user____str_lit_global_6c6162656c733a
    ref.as_non_null
  )
  (func $user____str_lit_get_6e6f6e2d65786861757374697665206d61746368 (type $functype_40)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_6e6f6e2d65786861757374697665206d61746368
    ref.is_null
    (if
      (then
        i32.const 110
        i32.const 111
        i32.const 110
        i32.const 45
        i32.const 101
        i32.const 120
        i32.const 104
        i32.const 97
        i32.const 117
        i32.const 115
        i32.const 116
        i32.const 105
        i32.const 118
        i32.const 101
        i32.const 32
        i32.const 109
        i32.const 97
        i32.const 116
        i32.const 99
        i32.const 104
        array.new_fixed $rt_types__String 20
        global.set $user____str_lit_global_6e6f6e2d65786861757374697665206d61746368))
    global.get $user____str_lit_global_6e6f6e2d65786861757374697665206d61746368
    ref.as_non_null
  )
  (func $__linked_init (type $functype_30)
    call $user____user_init
  )
  (elem declare func $user__func_45__closure $user__func_45__typed_closure $user__func_46__closure $user__func_46__typed_closure $user__func_47__closure $user__func_47__typed_closure)
  (export "rt_arr__make" (func $rt_arr__make))
  (export "rt_arr__get" (func $rt_arr__get))
  (export "rt_arr__set" (func $rt_arr__set))
  (export "rt_arr__len" (func $rt_arr__len))
  (export "rt_arr__concat" (func $rt_arr__concat))
  (export "rt_arr__slice" (func $rt_arr__slice))
  (export "rt_arr__builder_new" (func $rt_arr__builder_new))
  (export "rt_arr__builder_from" (func $rt_arr__builder_from))
  (export "rt_arr__builder_push" (func $rt_arr__builder_push))
  (export "rt_arr__builder_freeze" (func $rt_arr__builder_freeze))
  (export "rt_str__len" (func $rt_str__len))
  (export "rt_str__concat" (func $rt_str__concat))
  (export "rt_str__substring" (func $rt_str__substring))
  (export "rt_str__eq" (func $rt_str__eq))
  (export "rt_str__cmp" (func $rt_str__cmp))
  (export "rt_str__from_i64" (func $rt_str__from_i64))
  (export "rt_str__from_f64" (func $rt_str__from_f64))
  (export "rt_str__from_bool" (func $rt_str__from_bool))
  (export "rt_dict__make" (func $rt_dict__make))
  (export "rt_dict__len" (func $rt_dict__len))
  (export "rt_dict__keys" (func $rt_dict__keys))
  (export "rt_dict__has" (func $rt_dict__has))
  (export "rt_dict__get" (func $rt_dict__get))
  (export "rt_dict__get_option" (func $rt_dict__get_option))
  (export "rt_dict__set" (func $rt_dict__set))
  (export "rt_dict__remove" (func $rt_dict__remove))
  (export "rt_dict__set_in_place" (func $rt_dict__set_in_place))
  (export "rt_dict__remove_in_place" (func $rt_dict__remove_in_place))
  (export "rt_core__print" (func $rt_core__print))
  (export "rt_core__println" (func $rt_core__println))
  (export "rt_core__eprint" (func $rt_core__eprint))
  (export "rt_core__eprintln" (func $rt_core__eprintln))
  (export "rt_core__trap" (func $rt_core__trap))
  (export "rt_core__eq" (func $rt_core__eq))
  (start $__linked_init)
)