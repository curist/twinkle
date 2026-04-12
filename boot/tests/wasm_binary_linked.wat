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
  (type $rt_types__VecChildren (array (mut (ref null eq))))
  (type $rt_types__VecInternal (struct (field $children (ref $rt_types__VecChildren))))
  (type $rt_types__PVec (struct (field $len i32) (field $shift i32) (field $root (ref null $rt_types__VecInternal)) (field $tail (ref $rt_types__Array))))
  (type $rt_types__IterState (sub (struct (field $seed anyref) (field $step anyref))))
  (type $user__$Result_res_vec_i32b_str (sub (struct (field $tag i32) (field $Ok_0 (ref null $rt_types__PVec)) (field $Err_0 (ref null $rt_types__String)))))
  (type $functype_0 (func (param f64) (result (ref $rt_types__String))))
  (type $functype_1 (func (param (ref null $rt_types__String))))
  (type $functype_2 (func (param (ref null $rt_types__String)) (result f64 i32)))
  (type $functype_3 (func (param (ref null $rt_types__String)) (result i32)))
  (type $functype_4 (func (param (ref null $rt_types__String) (ref null $rt_types__String)) (result (ref $rt_types__String))))
  (type $functype_5 (func (param (ref null $rt_types__String) i32 i32) (result (ref $rt_types__String))))
  (type $functype_6 (func (param (ref null $rt_types__String) (ref null $rt_types__String)) (result i32)))
  (type $functype_7 (func (param i64) (result (ref $rt_types__String))))
  (type $functype_8 (func (param i32) (result (ref $rt_types__String))))
  (type $functype_9 (func (param i32) (result i32)))
  (type $functype_10 (func (param (ref $rt_types__PVec) i32) (result (ref $rt_types__Array))))
  (type $functype_11 (func (param i32 (ref eq)) (result (ref eq))))
  (type $functype_12 (func (param i32 i32 (ref null $rt_types__VecInternal) (ref eq)) (result (ref eq))))
  (type $functype_13 (func (param i32 (ref eq) i32 anyref) (result (ref eq))))
  (type $functype_14 (func (param (ref $rt_types__PVec) anyref) (result (ref $rt_types__PVec))))
  (type $functype_15 (func (param i32 anyref) (result (ref $rt_types__PVec))))
  (type $functype_16 (func (param (ref null $rt_types__PVec) i32) (result anyref)))
  (type $functype_17 (func (param (ref null $rt_types__PVec) i32 anyref) (result (ref $rt_types__PVec))))
  (type $functype_18 (func (param (ref null $rt_types__PVec)) (result i32)))
  (type $functype_19 (func (param (ref null $rt_types__PVec) (ref null $rt_types__PVec)) (result (ref $rt_types__PVec))))
  (type $functype_20 (func (param (ref null $rt_types__PVec) i32 i32) (result (ref $rt_types__PVec))))
  (type $functype_21 (func (result (ref $rt_types__Array))))
  (type $functype_22 (func (param (ref null $rt_types__PVec)) (result (ref $rt_types__Array))))
  (type $functype_23 (func (param (ref null $rt_types__Array) anyref)))
  (type $functype_24 (func (param (ref null $rt_types__Array) (ref null $rt_types__PVec))))
  (type $functype_25 (func (param (ref null $rt_types__Array)) (result (ref $rt_types__PVec))))
  (type $functype_26 (func (param (ref $rt_types__Array)) (result (ref $rt_types__PVec))))
  (type $functype_27 (func (param (ref null $rt_types__Variant)) (result (ref null $rt_types__Variant))))
  (type $functype_28 (func (result (ref $rt_types__Dict))))
  (type $functype_29 (func (param (ref null $rt_types__Dict)) (result i32)))
  (type $functype_30 (func (param (ref null $rt_types__Dict)) (result (ref $rt_types__PVec))))
  (type $functype_31 (func (param (ref null $rt_types__Dict) anyref) (result i32)))
  (type $functype_32 (func (param (ref null $rt_types__Dict) anyref) (result anyref)))
  (type $functype_33 (func (param (ref null $rt_types__Dict) anyref) (result (ref $rt_types__Variant))))
  (type $functype_34 (func (param (ref null $rt_types__Dict) anyref anyref) (result (ref $rt_types__Dict))))
  (type $functype_35 (func (param (ref null $rt_types__Dict) anyref) (result (ref $rt_types__Dict))))
  (type $functype_36 (func (param anyref anyref) (result i32)))
  (type $functype_37 (func (param (ref null $rt_types__String)) (result anyref)))
  (type $functype_38 (func (param i32) (result anyref)))
  (type $functype_39 (func (param (ref null $rt_types__String)) (result (ref $rt_types__Array))))
  (type $functype_40 (func (param (ref null $rt_types__Array)) (result anyref)))
  (type $functype_41 (func (param i64 i64) (result i64)))
  (type $functype_42 (func (param (ref null $rt_types__Variant)) (result (ref null $user__$Result_res_vec_i32b_str))))
  (type $functype_43 (func (param (ref null $rt_types__Variant)) (result (ref null $$Option_opt_str))))
  (type $functype_44 (func (param (ref null $rt_types__Variant)) (result (ref null $$Option_opt_i64))))
  (type $functype_45 (func (param (ref null $rt_types__Variant)) (result (ref null $$Option_opt_f64))))
  (import "host" "f64_to_string" (func $rt_str__host_f64_to_string (type $functype_0)))
  (import "host" "print" (func $rt_core__host_print (type $functype_1)))
  (import "host" "println" (func $rt_core__host_println (type $functype_1)))
  (import "host" "error" (func $rt_core__host_error (type $functype_1)))
  (import "host" "eprint" (func $rt_core__host_eprint (type $functype_1)))
  (import "host" "eprintln" (func $rt_core__host_eprintln (type $functype_1)))
  (import "host" "parse_float" (func $codegen_intrinsics__host_parse_float (type $functype_2)))
  (global $rt_arr__empty_leaf (ref $rt_types__Array) array.new_fixed $rt_types__Array 0)
  (global $rt_arr__empty_pvec (ref $rt_types__PVec) i32.const 0 i32.const 0 ref.null $rt_types__VecInternal global.get $rt_arr__empty_leaf struct.new $rt_types__PVec)
  (func $rt_str__len (type $functype_3)
    (param $p0 (ref null $rt_types__String))
    (result i32)
    local.get $p0
    ref.as_non_null
    array.len
  )
  (func $rt_str__concat (type $functype_4)
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
  (func $rt_str__substring (type $functype_5)
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
        local.set $p1
      )
    )
    local.get $p1
    local.get $p3
    i32.gt_s
    (if
      (then
        local.get $p3
        local.set $p1
      )
    )
    local.get $p2
    i32.const 0
    i32.lt_s
    (if
      (then
        local.get $p3
        local.set $p2
      )
    )
    local.get $p2
    local.get $p3
    i32.gt_s
    (if
      (then
        local.get $p3
        local.set $p2
      )
    )
    local.get $p2
    local.get $p1
    i32.lt_s
    (if
      (then
        local.get $p1
        local.set $p2
      )
    )
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
  (func $rt_str__eq (type $functype_6)
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
        return
      )
    )
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
            return
          )
        )
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $cmp
      )
    )
    i32.const 1
  )
  (func $rt_str__cmp (type $functype_6)
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
            return
          )
        )
        local.get $p4
        local.get $p5
        i32.gt_u
        (if
          (then
            i32.const 1
            return
          )
        )
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $cmp_loop
      )
    )
    local.get $p6
    local.get $p7
    i32.lt_s
    (if
      (then
        i32.const -1
        return
      )
    )
    local.get $p6
    local.get $p7
    i32.gt_s
    (if
      (then
        i32.const 1
        return
      )
    )
    i32.const 0
  )
  (func $rt_str__from_i64 (type $functype_7)
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
        return
      )
    )
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
        return
      )
    )
    local.get $p0
    i64.const 0
    i64.lt_s
    local.set $p1
    local.get $p1
    (if (result i64)
      (then
        i64.const 0
        local.get $p0
        i64.sub
      )
      (else
        local.get $p0
      )
    )
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
      br_if $digits
    )
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
        local.set $p3
      )
    )
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
  (func $rt_str__from_bool (type $functype_8)
    (param $p0 i32)
    (result (ref $rt_types__String))
    local.get $p0
    (if (result (ref $rt_types__String))
      (then
        i32.const 116
        i32.const 114
        i32.const 117
        i32.const 101
        array.new_fixed $rt_types__String 4
      )
      (else
        i32.const 102
        i32.const 97
        i32.const 108
        i32.const 115
        i32.const 101
        array.new_fixed $rt_types__String 5
      )
    )
  )
  (func $rt_arr__tailoff (type $functype_9)
    (param $p0 i32)
    (result i32)
    local.get $p0
    i32.const 32
    i32.le_s
    (if (result i32)
      (then
        i32.const 0
      )
      (else
        local.get $p0
        i32.const 1
        i32.sub
        i32.const 5
        i32.shr_u
        i32.const 5
        i32.shl
      )
    )
  )
  (func $rt_arr__get_leaf (type $functype_10)
    (param $p0 (ref $rt_types__PVec))
    (param $p1 i32)
    (result (ref $rt_types__Array))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__VecInternal))
    (local $p5 i32)
    local.get $p0
    struct.get $rt_types__PVec 0
    local.set $p2
    local.get $p2
    i32.const 32
    i32.le_s
    (if (result (ref $rt_types__Array))
      (then
        local.get $p0
        struct.get $rt_types__PVec 3
      )
      (else
        local.get $p2
        i32.const 1
        i32.sub
        i32.const 5
        i32.shr_u
        i32.const 5
        i32.shl
        local.set $p3
        local.get $p1
        local.get $p3
        i32.ge_s
        (if (result (ref $rt_types__Array))
          (then
            local.get $p0
            struct.get $rt_types__PVec 3
          )
          (else
            local.get $p0
            struct.get $rt_types__PVec 2
            local.set $p4
            local.get $p0
            struct.get $rt_types__PVec 1
            local.set $p5
            (block $brk
              (loop $lp
                local.get $p5
                i32.const 5
                i32.le_s
                br_if $brk
                local.get $p4
                ref.as_non_null
                struct.get $rt_types__VecInternal 0
                local.get $p1
                local.get $p5
                i32.shr_u
                i32.const 31
                i32.and
                array.get $rt_types__VecChildren
                ref.cast (ref null $rt_types__VecInternal)
                local.set $p4
                local.get $p5
                i32.const 5
                i32.sub
                local.set $p5
                br $lp
              )
            )
            local.get $p4
            ref.as_non_null
            struct.get $rt_types__VecInternal 0
            local.get $p1
            local.get $p5
            i32.shr_u
            i32.const 31
            i32.and
            array.get $rt_types__VecChildren
            ref.cast (ref $rt_types__Array)
          )
        )
      )
    )
  )
  (func $rt_arr__new_path (type $functype_11)
    (param $p0 i32)
    (param $p1 (ref eq))
    (result (ref eq))
    (local $p2 (ref null $rt_types__VecChildren))
    (block $brk
      (loop $lp
        local.get $p0
        i32.eqz
        br_if $brk
        ref.null eq
        i32.const 32
        array.new $rt_types__VecChildren
        local.set $p2
        local.get $p2
        ref.as_non_null
        i32.const 0
        local.get $p1
        array.set $rt_types__VecChildren
        local.get $p2
        ref.as_non_null
        struct.new $rt_types__VecInternal
        ref.cast (ref eq)
        local.set $p1
        local.get $p0
        i32.const 5
        i32.sub
        local.set $p0
        br $lp
      )
    )
    local.get $p1
  )
  (func $rt_arr__push_tail (type $functype_12)
    (param $p0 i32)
    (param $p1 i32)
    (param $p2 (ref null $rt_types__VecInternal))
    (param $p3 (ref eq))
    (result (ref eq))
    (local $p4 (ref null $rt_types__VecChildren))
    (local $p5 i32)
    (local $p6 (ref null eq))
    local.get $p0
    i32.const 1
    i32.sub
    local.get $p1
    i32.shr_u
    i32.const 31
    i32.and
    local.set $p5
    ref.null eq
    i32.const 32
    array.new $rt_types__VecChildren
    local.set $p4
    local.get $p4
    ref.as_non_null
    i32.const 0
    local.get $p2
    ref.as_non_null
    struct.get $rt_types__VecInternal 0
    i32.const 0
    i32.const 32
    array.copy $rt_types__VecChildren $rt_types__VecChildren
    local.get $p1
    i32.const 5
    i32.eq
    (if
      (then
        local.get $p4
        ref.as_non_null
        local.get $p5
        local.get $p3
        array.set $rt_types__VecChildren
      )
      (else
        local.get $p2
        ref.as_non_null
        struct.get $rt_types__VecInternal 0
        local.get $p5
        array.get $rt_types__VecChildren
        local.set $p6
        local.get $p6
        ref.is_null
        (if (result (ref eq))
          (then
            local.get $p1
            i32.const 5
            i32.sub
            local.get $p3
            call $rt_arr__new_path
          )
          (else
            local.get $p0
            local.get $p1
            i32.const 5
            i32.sub
            local.get $p6
            ref.cast (ref null $rt_types__VecInternal)
            local.get $p3
            call $rt_arr__push_tail
          )
        )
        local.set $p6
        local.get $p4
        ref.as_non_null
        local.get $p5
        local.get $p6
        array.set $rt_types__VecChildren
      )
    )
    local.get $p4
    ref.as_non_null
    struct.new $rt_types__VecInternal
    ref.cast (ref eq)
  )
  (func $rt_arr__do_set (type $functype_13)
    (param $p0 i32)
    (param $p1 (ref eq))
    (param $p2 i32)
    (param $p3 anyref)
    (result (ref eq))
    (local $p4 (ref null $rt_types__VecChildren))
    (local $p5 i32)
    (local $p6 (ref null $rt_types__Array))
    (local $p7 (ref null $rt_types__Array))
    local.get $p0
    i32.eqz
    (if (result (ref eq))
      (then
        local.get $p1
        ref.cast (ref $rt_types__Array)
        local.set $p7
        ref.null none
        local.get $p7
        ref.as_non_null
        array.len
        array.new $rt_types__Array
        local.set $p6
        local.get $p6
        ref.as_non_null
        i32.const 0
        local.get $p7
        ref.as_non_null
        i32.const 0
        local.get $p7
        ref.as_non_null
        array.len
        array.copy $rt_types__Array $rt_types__Array
        local.get $p6
        ref.as_non_null
        local.get $p2
        i32.const 31
        i32.and
        local.get $p3
        array.set $rt_types__Array
        local.get $p6
        ref.as_non_null
        ref.cast (ref eq)
      )
      (else
        local.get $p2
        local.get $p0
        i32.shr_u
        i32.const 31
        i32.and
        local.set $p5
        ref.null eq
        i32.const 32
        array.new $rt_types__VecChildren
        local.set $p4
        local.get $p4
        ref.as_non_null
        i32.const 0
        local.get $p1
        ref.cast (ref $rt_types__VecInternal)
        struct.get $rt_types__VecInternal 0
        i32.const 0
        i32.const 32
        array.copy $rt_types__VecChildren $rt_types__VecChildren
        local.get $p4
        ref.as_non_null
        local.get $p5
        local.get $p0
        i32.const 5
        i32.sub
        local.get $p1
        ref.cast (ref $rt_types__VecInternal)
        struct.get $rt_types__VecInternal 0
        local.get $p5
        array.get $rt_types__VecChildren
        ref.as_non_null
        local.get $p2
        local.get $p3
        call $rt_arr__do_set
        array.set $rt_types__VecChildren
        local.get $p4
        ref.as_non_null
        struct.new $rt_types__VecInternal
        ref.cast (ref eq)
      )
    )
  )
  (func $rt_arr__push (type $functype_14)
    (param $p0 (ref $rt_types__PVec))
    (param $p1 anyref)
    (result (ref $rt_types__PVec))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__Array))
    (local $p5 (ref null eq))
    (local $p6 i32)
    (local $p7 (ref null $rt_types__VecChildren))
    local.get $p0
    struct.get $rt_types__PVec 0
    local.set $p2
    local.get $p0
    struct.get $rt_types__PVec 3
    array.len
    local.set $p3
    local.get $p3
    i32.const 32
    i32.lt_s
    (if (result (ref $rt_types__PVec))
      (then
        ref.null none
        local.get $p3
        i32.const 1
        i32.add
        array.new $rt_types__Array
        local.set $p4
        local.get $p3
        i32.eqz
        (if
          (then
          )
          (else
            local.get $p4
            ref.as_non_null
            i32.const 0
            local.get $p0
            struct.get $rt_types__PVec 3
            i32.const 0
            local.get $p3
            array.copy $rt_types__Array $rt_types__Array
          )
        )
        local.get $p4
        ref.as_non_null
        local.get $p3
        local.get $p1
        array.set $rt_types__Array
        local.get $p2
        i32.const 1
        i32.add
        local.get $p0
        struct.get $rt_types__PVec 1
        local.get $p0
        struct.get $rt_types__PVec 2
        local.get $p4
        ref.as_non_null
        struct.new $rt_types__PVec
      )
      (else
        local.get $p0
        struct.get $rt_types__PVec 1
        local.set $p6
        local.get $p0
        struct.get $rt_types__PVec 2
        ref.is_null
        (if
          (then
            i32.const 5
            local.set $p6
            i32.const 5
            local.get $p0
            struct.get $rt_types__PVec 3
            ref.cast (ref eq)
            call $rt_arr__new_path
            local.set $p5
          )
          (else
            local.get $p2
            i32.const 5
            i32.shr_u
            i32.const 1
            local.get $p6
            i32.shl
            i32.gt_u
            (if
              (then
                ref.null eq
                i32.const 32
                array.new $rt_types__VecChildren
                local.set $p7
                local.get $p7
                ref.as_non_null
                i32.const 0
                local.get $p0
                struct.get $rt_types__PVec 2
                ref.as_non_null
                ref.cast (ref eq)
                array.set $rt_types__VecChildren
                local.get $p7
                ref.as_non_null
                i32.const 1
                local.get $p6
                local.get $p0
                struct.get $rt_types__PVec 3
                ref.cast (ref eq)
                call $rt_arr__new_path
                array.set $rt_types__VecChildren
                local.get $p7
                ref.as_non_null
                struct.new $rt_types__VecInternal
                ref.cast (ref eq)
                local.set $p5
                local.get $p6
                i32.const 5
                i32.add
                local.set $p6
              )
              (else
                local.get $p2
                local.get $p6
                local.get $p0
                struct.get $rt_types__PVec 2
                local.get $p0
                struct.get $rt_types__PVec 3
                ref.cast (ref eq)
                call $rt_arr__push_tail
                local.set $p5
              )
            )
          )
        )
        local.get $p1
        array.new_fixed $rt_types__Array 1
        local.set $p4
        local.get $p2
        i32.const 1
        i32.add
        local.get $p6
        local.get $p5
        ref.cast (ref null $rt_types__VecInternal)
        local.get $p4
        ref.as_non_null
        struct.new $rt_types__PVec
      )
    )
  )
  (func $rt_arr__make (type $functype_15)
    (param $p0 i32)
    (param $p1 anyref)
    (result (ref $rt_types__PVec))
    (local $p2 (ref null $rt_types__PVec))
    (local $p3 i32)
    local.get $p0
    i32.eqz
    (if (result (ref $rt_types__PVec))
      (then
        global.get $rt_arr__empty_pvec
      )
      (else
        global.get $rt_arr__empty_pvec
        local.set $p2
        i32.const 0
        local.set $p3
        (block $brk
          (loop $lp
            local.get $p3
            local.get $p0
            i32.ge_s
            br_if $brk
            local.get $p2
            ref.as_non_null
            local.get $p1
            call $rt_arr__push
            local.set $p2
            local.get $p3
            i32.const 1
            i32.add
            local.set $p3
            br $lp
          )
        )
        local.get $p2
        ref.as_non_null
      )
    )
  )
  (func $rt_arr__get (type $functype_16)
    (param $p0 (ref null $rt_types__PVec))
    (param $p1 i32)
    (result anyref)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__VecInternal))
    (local $p5 i32)
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 0
    local.set $p2
    local.get $p2
    i32.const 32
    i32.le_s
    (if (result anyref)
      (then
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 3
        local.get $p1
        i32.const 31
        i32.and
        array.get $rt_types__Array
      )
      (else
        local.get $p2
        i32.const 1
        i32.sub
        i32.const 5
        i32.shr_u
        i32.const 5
        i32.shl
        local.set $p3
        local.get $p1
        local.get $p3
        i32.ge_s
        (if (result anyref)
          (then
            local.get $p0
            ref.as_non_null
            struct.get $rt_types__PVec 3
            local.get $p1
            i32.const 31
            i32.and
            array.get $rt_types__Array
          )
          (else
            local.get $p0
            ref.as_non_null
            struct.get $rt_types__PVec 2
            local.set $p4
            local.get $p0
            ref.as_non_null
            struct.get $rt_types__PVec 1
            local.set $p5
            (block $brk
              (loop $lp
                local.get $p5
                i32.const 5
                i32.le_s
                br_if $brk
                local.get $p4
                ref.as_non_null
                struct.get $rt_types__VecInternal 0
                local.get $p1
                local.get $p5
                i32.shr_u
                i32.const 31
                i32.and
                array.get $rt_types__VecChildren
                ref.cast (ref null $rt_types__VecInternal)
                local.set $p4
                local.get $p5
                i32.const 5
                i32.sub
                local.set $p5
                br $lp
              )
            )
            local.get $p4
            ref.as_non_null
            struct.get $rt_types__VecInternal 0
            local.get $p1
            local.get $p5
            i32.shr_u
            i32.const 31
            i32.and
            array.get $rt_types__VecChildren
            ref.cast (ref $rt_types__Array)
            local.get $p1
            i32.const 31
            i32.and
            array.get $rt_types__Array
          )
        )
      )
    )
  )
  (func $rt_arr__set (type $functype_17)
    (param $p0 (ref null $rt_types__PVec))
    (param $p1 i32)
    (param $p2 anyref)
    (result (ref $rt_types__PVec))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 0
    call $rt_arr__tailoff
    local.set $p3
    local.get $p1
    local.get $p3
    i32.ge_s
    (if (result (ref $rt_types__PVec))
      (then
        ref.null none
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 3
        array.len
        array.new $rt_types__Array
        local.set $p4
        local.get $p4
        ref.as_non_null
        i32.const 0
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 3
        i32.const 0
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 3
        array.len
        array.copy $rt_types__Array $rt_types__Array
        local.get $p4
        ref.as_non_null
        local.get $p1
        local.get $p3
        i32.sub
        local.get $p2
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 0
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 2
        local.get $p4
        ref.as_non_null
        struct.new $rt_types__PVec
      )
      (else
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 0
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 2
        ref.as_non_null
        ref.cast (ref eq)
        local.get $p1
        local.get $p2
        call $rt_arr__do_set
        ref.cast (ref null $rt_types__VecInternal)
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 3
        struct.new $rt_types__PVec
      )
    )
  )
  (func $rt_arr__len (type $functype_18)
    (param $p0 (ref null $rt_types__PVec))
    (result i32)
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 0
  )
  (func $rt_arr__concat (type $functype_19)
    (param $p0 (ref null $rt_types__PVec))
    (param $p1 (ref null $rt_types__PVec))
    (result (ref $rt_types__PVec))
    (local $p2 (ref null $rt_types__PVec))
    (local $p3 i32)
    (local $p4 i32)
    local.get $p0
    ref.as_non_null
    local.set $p2
    local.get $p1
    ref.as_non_null
    struct.get $rt_types__PVec 0
    local.set $p4
    i32.const 0
    local.set $p3
    (block $brk
      (loop $lp
        local.get $p3
        local.get $p4
        i32.ge_s
        br_if $brk
        local.get $p2
        ref.as_non_null
        local.get $p1
        local.get $p3
        call $rt_arr__get
        call $rt_arr__push
        local.set $p2
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $lp
      )
    )
    local.get $p2
    ref.as_non_null
  )
  (func $rt_arr__slice (type $functype_20)
    (param $p0 (ref null $rt_types__PVec))
    (param $p1 i32)
    (param $p2 i32)
    (result (ref $rt_types__PVec))
    (local $p3 (ref null $rt_types__PVec))
    (local $p4 i32)
    global.get $rt_arr__empty_pvec
    local.set $p3
    local.get $p1
    local.set $p4
    (block $brk
      (loop $lp
        local.get $p4
        local.get $p2
        i32.ge_s
        br_if $brk
        local.get $p3
        ref.as_non_null
        local.get $p0
        local.get $p4
        call $rt_arr__get
        call $rt_arr__push
        local.set $p3
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $lp
      )
    )
    local.get $p3
    ref.as_non_null
  )
  (func $rt_arr__builder_new (type $functype_21)
    (result (ref $rt_types__Array))
    global.get $rt_arr__empty_pvec
    i64.const 0
    struct.new $rt_types__BoxedInt
    ref.null none
    i32.const 32
    array.new $rt_types__Array
    array.new_fixed $rt_types__Array 3
  )
  (func $rt_arr__builder_from (type $functype_22)
    (param $p0 (ref null $rt_types__PVec))
    (result (ref $rt_types__Array))
    (local $p1 (ref null $rt_types__Array))
    (local $p2 i32)
    (local $p3 (ref null $rt_types__Array))
    (local $p4 (ref null $rt_types__PVec))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 3
    local.set $p1
    local.get $p1
    ref.as_non_null
    array.len
    local.set $p2
    ref.null none
    i32.const 32
    array.new $rt_types__Array
    local.set $p3
    local.get $p2
    i32.eqz
    (if
      (then
      )
      (else
        local.get $p3
        ref.as_non_null
        i32.const 0
        local.get $p1
        ref.as_non_null
        i32.const 0
        local.get $p2
        array.copy $rt_types__Array $rt_types__Array
      )
    )
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 0
    local.get $p2
    i32.sub
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 1
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 2
    global.get $rt_arr__empty_leaf
    struct.new $rt_types__PVec
    local.set $p4
    local.get $p4
    ref.as_non_null
    local.get $p2
    i64.extend_i32_s
    struct.new $rt_types__BoxedInt
    local.get $p3
    array.new_fixed $rt_types__Array 3
  )
  (func $rt_arr__builder_push (type $functype_23)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 anyref)
    (local $p2 (ref null $rt_types__Array))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__PVec))
    (local $p5 (ref null $rt_types__PVec))
    local.get $p0
    ref.as_non_null
    i32.const 2
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
    local.get $p3
    i32.const 32
    i32.lt_s
    (if
      (then
        local.get $p2
        ref.as_non_null
        local.get $p3
        local.get $p1
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        i32.const 1
        local.get $p3
        i32.const 1
        i32.add
        i64.extend_i32_s
        struct.new $rt_types__BoxedInt
        array.set $rt_types__Array
      )
      (else
        local.get $p0
        ref.as_non_null
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref null $rt_types__PVec)
        local.set $p4
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__PVec 0
        i32.const 32
        i32.add
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__PVec 2
        local.get $p2
        ref.as_non_null
        struct.new $rt_types__PVec
        local.get $p1
        call $rt_arr__push
        local.set $p5
        local.get $p0
        ref.as_non_null
        i32.const 0
        local.get $p5
        ref.as_non_null
        struct.get $rt_types__PVec 0
        i32.const 1
        i32.sub
        local.get $p5
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p5
        ref.as_non_null
        struct.get $rt_types__PVec 2
        global.get $rt_arr__empty_leaf
        struct.new $rt_types__PVec
        array.set $rt_types__Array
        ref.null none
        i32.const 32
        array.new $rt_types__Array
        local.set $p2
        local.get $p2
        ref.as_non_null
        i32.const 0
        local.get $p1
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        i32.const 2
        local.get $p2
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        i32.const 1
        i64.const 1
        struct.new $rt_types__BoxedInt
        array.set $rt_types__Array
      )
    )
  )
  (func $rt_arr__builder_extend (type $functype_24)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 (ref null $rt_types__PVec))
    (local $p2 i32)
    (local $p3 i32)
    local.get $p1
    ref.as_non_null
    struct.get $rt_types__PVec 0
    local.set $p3
    i32.const 0
    local.set $p2
    (block $brk
      (loop $lp
        local.get $p2
        local.get $p3
        i32.ge_s
        br_if $brk
        local.get $p0
        local.get $p1
        local.get $p2
        call $rt_arr__get
        call $rt_arr__builder_push
        local.get $p2
        i32.const 1
        i32.add
        local.set $p2
        br $lp
      )
    )
  )
  (func $rt_arr__builder_freeze (type $functype_25)
    (param $p0 (ref null $rt_types__Array))
    (result (ref $rt_types__PVec))
    (local $p1 (ref null $rt_types__PVec))
    (local $p2 i32)
    (local $p3 (ref null $rt_types__Array))
    (local $p4 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__PVec)
    local.set $p1
    local.get $p0
    ref.as_non_null
    i32.const 1
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    i32.wrap_i64
    local.set $p2
    local.get $p0
    ref.as_non_null
    i32.const 2
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__Array)
    local.set $p3
    local.get $p2
    i32.eqz
    (if (result (ref $rt_types__PVec))
      (then
        local.get $p1
        ref.as_non_null
      )
      (else
        ref.null none
        local.get $p2
        array.new $rt_types__Array
        local.set $p4
        local.get $p4
        ref.as_non_null
        i32.const 0
        local.get $p3
        ref.as_non_null
        i32.const 0
        local.get $p2
        array.copy $rt_types__Array $rt_types__Array
        local.get $p1
        ref.as_non_null
        struct.get $rt_types__PVec 0
        local.get $p2
        i32.add
        local.get $p1
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p1
        ref.as_non_null
        struct.get $rt_types__PVec 2
        local.get $p4
        ref.as_non_null
        struct.new $rt_types__PVec
      )
    )
  )
  (func $rt_arr__from_array (type $functype_26)
    (param $p0 (ref $rt_types__Array))
    (result (ref $rt_types__PVec))
    (local $p1 i32)
    (local $p2 (ref null $rt_types__PVec))
    (local $p3 i32)
    local.get $p0
    array.len
    local.set $p1
    local.get $p1
    i32.eqz
    (if (result (ref $rt_types__PVec))
      (then
        global.get $rt_arr__empty_pvec
      )
      (else
        local.get $p1
        i32.const 32
        i32.le_s
        (if (result (ref $rt_types__PVec))
          (then
            local.get $p1
            i32.const 0
            ref.null $rt_types__VecInternal
            local.get $p0
            struct.new $rt_types__PVec
          )
          (else
            global.get $rt_arr__empty_pvec
            local.set $p2
            i32.const 0
            local.set $p3
            (block $brk
              (loop $lp
                local.get $p3
                local.get $p1
                i32.ge_s
                br_if $brk
                local.get $p2
                ref.as_non_null
                local.get $p0
                local.get $p3
                array.get $rt_types__Array
                call $rt_arr__push
                local.set $p2
                local.get $p3
                i32.const 1
                i32.add
                local.set $p3
                br $lp
              )
            )
            local.get $p2
            ref.as_non_null
          )
        )
      )
    )
  )
  (func $rt_arr__to_array (type $functype_22)
    (param $p0 (ref null $rt_types__PVec))
    (result (ref $rt_types__Array))
    (local $p1 i32)
    (local $p2 (ref null $rt_types__Array))
    (local $p3 i32)
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 0
    local.set $p1
    ref.null none
    local.get $p1
    array.new $rt_types__Array
    local.set $p2
    i32.const 0
    local.set $p3
    (block $brk
      (loop $lp
        local.get $p3
        local.get $p1
        i32.ge_s
        br_if $brk
        local.get $p2
        ref.as_non_null
        local.get $p3
        local.get $p0
        ref.as_non_null
        local.get $p3
        call $rt_arr__get
        array.set $rt_types__Array
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $lp
      )
    )
    local.get $p2
    ref.as_non_null
  )
  (func $rt_arr__from_read_file_result (type $functype_27)
    (param $p0 (ref null $rt_types__Variant))
    (result (ref null $rt_types__Variant))
    (local $p1 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__Variant 0
    i32.const 1
    i32.eq
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__Variant 1
        i32.eqz
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p0
            ref.as_non_null
            struct.get $rt_types__Variant 2
            local.set $p1
            i32.const 1
            i32.const 0
            local.get $p1
            ref.as_non_null
            i32.const 0
            array.get $rt_types__Array
            ref.cast (ref $rt_types__Array)
            call $rt_arr__from_array
            array.new_fixed $rt_types__Array 1
            struct.new $rt_types__Variant
          )
          (else
            local.get $p0
          )
        )
      )
      (else
        local.get $p0
      )
    )
  )
  (func $rt_dict__make (type $functype_28)
    (result (ref $rt_types__Dict))
    array.new_fixed $rt_types__Dict 0
  )
  (func $rt_dict__len (type $functype_29)
    (param $p0 (ref null $rt_types__Dict))
    (result i32)
    local.get $p0
    ref.as_non_null
    array.len
  )
  (func $rt_dict__keys (type $functype_30)
    (param $p0 (ref null $rt_types__Dict))
    (result (ref $rt_types__PVec))
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
        br $loop
      )
    )
    local.get $p3
    ref.as_non_null
    call $rt_arr__from_array
  )
  (func $rt_dict__has (type $functype_31)
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
            return
          )
        )
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan
      )
    )
    i32.const 0
  )
  (func $rt_dict__get (type $functype_32)
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
            return
          )
        )
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan
      )
    )
    ref.null any
  )
  (func $rt_dict__get_option (type $functype_33)
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
            return
          )
        )
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan
      )
    )
    i32.const 0
    i32.const 0
    ref.null $rt_types__Array
    struct.new $rt_types__Variant
  )
  (func $rt_dict__set (type $functype_34)
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
            br $found_exit
          )
        )
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $scan
      )
    )
    local.get $p5
    (if (result i32)
      (then
        local.get $p3
      )
      (else
        local.get $p3
        i32.const 1
        i32.add
      )
    )
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
            array.set $rt_types__Dict
          )
          (else
            local.get $p6
            ref.as_non_null
            local.get $p4
            local.get $p8
            array.set $rt_types__Dict
          )
        )
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $copy
      )
    )
    local.get $p5
    i32.eqz
    (if
      (then
        local.get $p6
        ref.as_non_null
        local.get $p3
        local.get $p7
        array.set $rt_types__Dict
      )
    )
    local.get $p6
    ref.as_non_null
  )
  (func $rt_dict__remove (type $functype_35)
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
            br $scan_exit
          )
        )
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan
      )
    )
    local.get $p7
    (if (result i32)
      (then
        local.get $p2
        i32.const 1
        i32.sub
      )
      (else
        local.get $p2
      )
    )
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
          (then
          )
          (else
            local.get $p5
            ref.as_non_null
            local.get $p4
            local.get $p6
            array.set $rt_types__Dict
            local.get $p4
            i32.const 1
            i32.add
            local.set $p4
          )
        )
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $copy
      )
    )
    local.get $p5
    ref.as_non_null
  )
  (func $rt_dict__set_in_place (type $functype_34)
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
            return
          )
        )
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $scan
      )
    )
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
  (func $rt_dict__remove_in_place (type $functype_35)
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
            br $scan_exit
          )
        )
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $scan
      )
    )
    local.get $p4
    i32.const 1
    i32.add
    i32.eqz
    (if
      (then
        local.get $p0
        ref.as_non_null
        return
      )
    )
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
        array.copy $rt_types__Dict $rt_types__Dict
      )
    )
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
        array.copy $rt_types__Dict $rt_types__Dict
      )
    )
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
  (func $rt_core__eq (type $functype_36)
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
        return
      )
    )
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
        return
      )
    )
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
        return
      )
    )
    local.get $p0
    ref.cast (ref null eq)
    local.get $p1
    ref.cast (ref null eq)
    ref.eq
  )
  (func $codegen_intrinsics__int_from_string_helper (type $functype_37)
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
        local.set $p6
      )
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
                local.set $p6
              )
            )
          )
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
                    local.set $p6
                  )
                )
              )
              (else
                i32.const 0
                local.set $p2
              )
            )
          )
        )
        local.get $p6
        (if
          (then
            (block $done_int_parse
              (loop $digit_loop
                local.get $p2
                local.get $p3
                i32.ge_s
                br_if $done_int_parse
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
                    br $done_int_parse
                  )
                )
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
                br $digit_loop
              )
            )
          )
        )
      )
    )
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
        struct.new $rt_types__Variant
      )
      (else
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant
      )
    )
  )
  (func $codegen_intrinsics__from_code_point_helper (type $functype_38)
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
        struct.new $rt_types__Variant
      )
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
            struct.new $rt_types__Variant
          )
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
                struct.new $rt_types__Variant
              )
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
                    struct.new $rt_types__Variant
                  )
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
                        struct.new $rt_types__Variant
                      )
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
                            struct.new $rt_types__Variant
                          )
                          (else
                            i32.const 0
                            i32.const 0
                            array.new_fixed $rt_types__Array 0
                            struct.new $rt_types__Variant
                          )
                        )
                      )
                    )
                  )
                )
              )
            )
          )
        )
      )
    )
  )
  (func $codegen_intrinsics__string_utf8_bytes_helper (type $functype_39)
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
    (block $utf8_bytes_break
      (loop $utf8_bytes_continue
        local.get $p2
        local.get $p1
        i32.ge_u
        br_if $utf8_bytes_break
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
        br $utf8_bytes_continue
      )
    )
    local.get $p3
  )
  (func $codegen_intrinsics__string_from_utf8_helper (type $functype_40)
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
    (block $utf8_validate_break
      (loop $utf8_validate_continue
        local.get $p2
        local.get $p1
        i32.ge_u
        br_if $utf8_validate_break
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
            local.set $p2
          )
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
                    br $utf8_validate_break
                  )
                )
                local.get $p2
                i32.const 1
                i32.add
                local.get $p1
                i32.ge_u
                (if
                  (then
                    i32.const 0
                    local.set $p5
                    br $utf8_validate_break
                  )
                )
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
                    br $utf8_validate_break
                  )
                )
                local.get $p2
                i32.const 2
                i32.add
                local.set $p2
              )
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
                        br $utf8_validate_break
                      )
                    )
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
                        br $utf8_validate_break
                      )
                    )
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
                        br $utf8_validate_break
                      )
                    )
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
                            br $utf8_validate_break
                          )
                        )
                      )
                    )
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
                            br $utf8_validate_break
                          )
                        )
                      )
                    )
                    local.get $p2
                    i32.const 3
                    i32.add
                    local.set $p2
                  )
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
                            br $utf8_validate_break
                          )
                        )
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
                            br $utf8_validate_break
                          )
                        )
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
                            br $utf8_validate_break
                          )
                        )
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
                            br $utf8_validate_break
                          )
                        )
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
                                br $utf8_validate_break
                              )
                            )
                          )
                        )
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
                                br $utf8_validate_break
                              )
                            )
                          )
                        )
                        local.get $p2
                        i32.const 4
                        i32.add
                        local.set $p2
                      )
                      (else
                        i32.const 0
                        local.set $p5
                        br $utf8_validate_break
                      )
                    )
                  )
                )
              )
            )
          )
        )
        br $utf8_validate_continue
      )
    )
    local.get $p5
    i32.eqz
    (if (result anyref)
      (then
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant
      )
      (else
        local.get $p1
        array.new_default $rt_types__String
        local.set $p3
        i32.const 0
        local.set $p2
        (block $utf8_copy_break
          (loop $utf8_copy_continue
            local.get $p2
            local.get $p1
            i32.ge_u
            br_if $utf8_copy_break
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
            br $utf8_copy_continue
          )
        )
        i32.const 0
        i32.const 1
        local.get $p3
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant
      )
    )
  )
  (func $codegen_intrinsics__float_from_string_helper (type $functype_37)
    (param $p0 (ref null $rt_types__String))
    (result anyref)
    (local $p1 f64)
    (local $p2 i32)
    local.get $p0
    call $codegen_intrinsics__host_parse_float
    local.set $p2
    local.set $p1
    local.get $p2
    (if (result anyref)
      (then
        i32.const 0
        i32.const 1
        local.get $p1
        struct.new $rt_types__BoxedFloat
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant
      )
      (else
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant
      )
    )
  )
  (func $user__$f106_97100100 (type $functype_41)
    (param $p0 i64)
    (param $p1 i64)
    (result i64)
    (local $p2 i64)
    local.get $p0
    local.get $p1
    i64.add
    local.set $p2
    local.get $p2
  )
  (func $user__host_read_file_result_helper (type $functype_42)
    (param $p0 (ref null $rt_types__Variant))
    (result (ref null $user__$Result_res_vec_i32b_str))
    local.get $p0
    ref.cast (ref null $rt_types__Variant)
    struct.get $rt_types__Variant 1
    i32.eqz
    (if (result (ref null $user__$Result_res_vec_i32b_str))
      (then
        i32.const 0
        local.get $p0
        struct.get $rt_types__Variant 2
        ref.cast (ref null $rt_types__Array)
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref null $rt_types__PVec)
        ref.null $rt_types__String
        struct.new $user__$Result_res_vec_i32b_str
      )
      (else
        i32.const 1
        ref.null $rt_types__PVec
        local.get $p0
        struct.get $rt_types__Variant 2
        ref.cast (ref null $rt_types__Array)
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref null $rt_types__String)
        struct.new $user__$Result_res_vec_i32b_str
      )
    )
  )
  (func $user__option_from_variant_helper_str (type $functype_43)
    (param $p0 (ref null $rt_types__Variant))
    (result (ref null $$Option_opt_str))
    local.get $p0
    struct.get $rt_types__Variant 1
    i32.eqz
    (if (result (ref null $$Option_opt_str))
      (then
        i32.const 0
        ref.null $rt_types__String
        struct.new $$Option_opt_str
      )
      (else
        i32.const 1
        local.get $p0
        struct.get $rt_types__Variant 2
        ref.cast (ref null $rt_types__Array)
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref null $rt_types__String)
        struct.new $$Option_opt_str
      )
    )
  )
  (func $user__option_from_variant_helper_i64 (type $functype_44)
    (param $p0 (ref null $rt_types__Variant))
    (result (ref null $$Option_opt_i64))
    local.get $p0
    struct.get $rt_types__Variant 1
    i32.eqz
    (if (result (ref null $$Option_opt_i64))
      (then
        i32.const 0
        i64.const 0
        struct.new $$Option_opt_i64
      )
      (else
        i32.const 1
        local.get $p0
        struct.get $rt_types__Variant 2
        ref.cast (ref null $rt_types__Array)
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref $rt_types__BoxedInt)
        struct.get $rt_types__BoxedInt 0
        struct.new $$Option_opt_i64
      )
    )
  )
  (func $user__option_from_variant_helper_f64 (type $functype_45)
    (param $p0 (ref null $rt_types__Variant))
    (result (ref null $$Option_opt_f64))
    local.get $p0
    struct.get $rt_types__Variant 1
    i32.eqz
    (if (result (ref null $$Option_opt_f64))
      (then
        i32.const 0
        f64.const 0
        struct.new $$Option_opt_f64
      )
      (else
        i32.const 1
        local.get $p0
        struct.get $rt_types__Variant 2
        ref.cast (ref null $rt_types__Array)
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref $rt_types__BoxedFloat)
        struct.get $rt_types__BoxedFloat 0
        struct.new $$Option_opt_f64
      )
    )
  )
  (export "int_from_string_helper" (func $codegen_intrinsics__int_from_string_helper))
  (export "from_code_point_helper" (func $codegen_intrinsics__from_code_point_helper))
  (export "string_utf8_bytes_helper" (func $codegen_intrinsics__string_utf8_bytes_helper))
  (export "string_from_utf8_helper" (func $codegen_intrinsics__string_from_utf8_helper))
  (export "float_from_string_helper" (func $codegen_intrinsics__float_from_string_helper))
)
