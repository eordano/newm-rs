(* src/index.rs flood and src/place.rs islands, splits, is_bridge and Search::splits: islands are
   a flood through the windows sharing a side, found in the range tree, and each check built on
   them answers what Canvas.splits and Canvas.is_bridge (connected, by grow) answer. *)
From Stdlib Require Import ZArith List Bool Lia Permutation.
Import ListNotations.
From NewmCanvas Require Import Canvas Push Index.
From NewmCanvas Require Claims.
Open Scope Z_scope.

(* index::flood: part holds each position's island as Some (the position the flood started
   from), None (usize::MAX) before it is reached; the stack's head is its top. *)
Definition get (p : list (option nat)) (b : nat) := nth b p None.
Fixpoint put (p : list (option nat)) (b : nat) (v : option nat) : list (option nat) :=
  match p, b with
  | [], _ => []
  | _ :: p', O => v :: p'
  | x :: p', S b' => x :: put p' b' v
  end.
Definition visit (s : nat) (st : list (option nat) * list nat) (b : nat) :=
  let '(p, stk) := st in
  match get p b with None => (put p b (Some s), b :: stk) | Some _ => st end.
Fixpoint drain (fuel : nat) (nb : nat -> list nat) (s : nat) (p : list (option nat)) (stk : list nat) :=
  match fuel with
  | O => p
  | S f => match stk with
           | [] => p
           | a :: rest => let '(p', stk') := fold_left (visit s) (nb a) (p, rest) in drain f nb s p' stk'
           end
  end.
Definition flood (n : nat) (nb : nat -> list nat) :=
  fold_left (fun p s => match get p s with
                        | Some _ => p
                        | None => drain (S n) nb s (put p s (Some s)) [s]
                        end) (seq 0 n) (repeat None n).

(* Reachable through adj among positions below n. *)
Inductive reach (n : nat) (adj : nat -> nat -> bool) (a : nat) : nat -> Prop :=
  | reach_refl : reach n adj a a
  | reach_step b c : reach n adj a b -> (c < n)%nat -> adj b c = true -> reach n adj a c.

(* ---------- The flood: put/get, reach, and the invariant of each start ---------- *)

Lemma put_length p b v : length (put p b v) = length p.
Proof. revert b; induction p as [|x p IH]; intros [|b]; simpl; rewrite ?IH; reflexivity. Qed.

Lemma get_put_same p b v : (b < length p)%nat -> get (put p b v) b = v.
Proof. unfold get. revert b; induction p as [|x p IH]; intros [|b] H; simpl in *; try (exfalso; lia); auto.
  apply IH. lia.
Qed.

Lemma get_put_other p b c v : b <> c -> get (put p b v) c = get p c.
Proof.
  unfold get. revert b c; induction p as [|x p IH]; intros [|b] [|c] H; simpl; try reflexivity; try congruence.
  apply IH. congruence.
Qed.

Definition nones (p : list (option nat)) := length (filter (fun o => match o with None => true | Some _ => false end) p).

Lemma nones_put p b s : get p b = None -> (b < length p)%nat -> S (nones (put p b (Some s))) = nones p.
Proof. unfold get, nones. revert b; induction p as [|x p IH]; intros [|b] G L; simpl in *; try (exfalso; lia).
  - subst x. reflexivity.
  - destruct x; simpl; [|f_equal]; apply IH; auto; lia.
Qed.

Lemma reach_lt n adj a b : reach n adj a b -> (a < n)%nat -> (b < n)%nat.
Proof. induction 1; auto. Qed.

Lemma reach_trans n adj a b c : reach n adj a b -> reach n adj b c -> reach n adj a c.
Proof. intros H1 H2. induction H2 as [|c d H2 IH Hd E]; [exact H1 | exact (reach_step n adj a c d IH Hd E)]. Qed.

Lemma reach_sym n adj a b : (forall x y, adj x y = adj y x) -> (a < n)%nat -> reach n adj a b -> reach n adj b a.
Proof. intros Hs Ha H. induction H as [|b c H IH Hc E]; [constructor|].
  apply (reach_trans n adj c b a); [|exact IH].
  apply (reach_step n adj c c b); [constructor | apply (reach_lt n adj a b H Ha) | rewrite Hs; exact E].
Qed.

(* Every label is a start that reaches its position; a labelled position off the pending list has
   all its neighbours under its label; the pending positions carry the current start. *)
Definition Inv n adj (s : nat) (pend : list nat) (p : list (option nat)) :=
  length p = n /\
  (forall y t, get p y = Some t -> (t < n)%nat /\ reach n adj t y) /\
  (forall y t, get p y = Some t -> ~ In y pend -> forall z, (z < n)%nat -> adj y z = true -> get p z = Some t) /\
  (forall y, In y pend -> (y < n)%nat /\ get p y = Some s).

Definition ext (p p' : list (option nat)) := forall y t, get p y = Some t -> get p' y = Some t.

Lemma visit_eq s p stk b :
  visit s (p, stk) b = match get p b with None => (put p b (Some s), b :: stk) | Some _ => (p, stk) end.
Proof. reflexivity. Qed.

Lemma visit_fold n adj s a (Hs : forall x y, adj x y = adj y x) : forall l p stk p' stk',
  Inv n adj s (a :: stk) p -> (forall b, In b l -> (b < n)%nat /\ adj a b = true) ->
  fold_left (visit s) l (p, stk) = (p', stk') ->
  Inv n adj s (a :: stk') p' /\ ext p p' /\ (length stk' + nones p' = length stk + nones p)%nat /\
  (forall b, In b l -> get p' b = Some s).
Proof. induction l as [|b l IH]; intros p stk p' stk' I Hl F.
  - simpl in F. injection F as <- <-.
    split; [exact I | split; [intros ? ? G; exact G | split; [reflexivity | intros ? []]]].
  - cbn [fold_left] in F. rewrite visit_eq in F. pose proof I as (Ln & Lab & Cl & Pd).
    destruct (Pd a (or_introl eq_refl)) as [Ha Pa]. destruct (Hl b (or_introl eq_refl)) as [Hb Eab].
    assert (Hl' : forall c, In c l -> (c < n)%nat /\ adj a c = true) by (intros c Hc; apply Hl; right; exact Hc).
    revert F. destruct (get p b) as [t|] eqn:G; intros F.
    + assert (t = s) as ->.
      { destruct (in_dec Nat.eq_dec b (a :: stk)) as [Hin|Hin].
        - destruct (Pd b Hin) as [_ G']. congruence.
        - pose proof (Cl b t G Hin a Ha) as C. rewrite Hs in C. specialize (C Eab). congruence. }
      destruct (IH p stk p' stk' I Hl' F) as (I' & E' & M' & A').
      split; [exact I' | split; [exact E' | split; [exact M' |]]]. intros c [<-|Hc]; [apply E', G | apply A', Hc].
    + assert (E1 : ext p (put p b (Some s))).
      { intros y t Gy. rewrite get_put_other; [exact Gy | congruence]. }
      assert (I1 : Inv n adj s (a :: b :: stk) (put p b (Some s))).
      { split; [rewrite put_length; exact Ln | split; [|split]].
        - intros y t Gy. destruct (Nat.eq_dec y b) as [->|Ny].
          + rewrite get_put_same in Gy by lia. injection Gy as <-. destruct (Lab a s Pa) as [Hs' Ra].
            split; [exact Hs' | apply (reach_step _ _ _ a); auto].
          + rewrite get_put_other in Gy by congruence. apply (Lab y t Gy).
        - intros y t Gy Ny z Hz Ez. rewrite get_put_other in Gy by (intros ->; apply Ny; right; left; reflexivity).
          apply E1, (Cl y t Gy); auto. intros [Hy|Hy]; apply Ny; [left; exact Hy | right; right; exact Hy].
        - intros y [<-|[<-|Hy]].
          + split; [exact Ha | apply E1, Pa].
          + split; [exact Hb | apply get_put_same; lia].
          + destruct (Pd y (or_intror Hy)) as [Hy1 Hy2]. split; [exact Hy1 | apply E1, Hy2]. }
      destruct (IH _ _ p' stk' I1 Hl' F) as (I' & E' & M' & A').
      split; [exact I' | split; [intros y t Gy; apply E', E1, Gy | split]].
      * simpl in M'. pose proof (nones_put p b s G ltac:(lia)). lia.
      * intros c [<-|Hc]; [apply E', get_put_same; lia | apply A', Hc].
Qed.

Lemma drain_inv n nb adj s (Hs : forall x y, adj x y = adj y x)
  (Hnb : forall a, (a < n)%nat -> forall b, In b (nb a) <-> (b < n)%nat /\ adj a b = true) :
  forall fuel p stk, Inv n adj s stk p -> (length stk + nones p <= fuel)%nat ->
  Inv n adj s [] (drain fuel nb s p stk) /\ ext p (drain fuel nb s p stk).
Proof. induction fuel as [|f IH]; intros p stk I M.
  - destruct stk; simpl in M; [|exfalso; lia]. split; [exact I | intros ? ? G; exact G].
  - destruct stk as [|a rest]; [split; [exact I | intros ? ? G; exact G]|].
    cbn [drain]. destruct (fold_left (visit s) (nb a) (p, rest)) as [p1 stk1] eqn:F. cbv beta iota.
    pose proof I as (Ln & Lab & Cl & Pd). destruct (Pd a (or_introl eq_refl)) as [Ha Pa].
    destruct (visit_fold n adj s a Hs (nb a) p rest p1 stk1 I (fun b Hb => proj1 (Hnb a Ha b) Hb) F)
      as (I1 & E1 & M1 & A1).
    assert (I2 : Inv n adj s stk1 p1).
    { destruct I1 as (Ln1 & Lab1 & Cl1 & Pd1). split; [exact Ln1 | split; [exact Lab1 | split]].
      - intros y t G Ny z Hz Ez. destruct (Nat.eq_dec y a) as [->|Na].
        + rewrite (E1 a s Pa) in G. injection G as <-. apply A1, (Hnb a Ha). split; assumption.
        + apply (Cl1 y t G); auto. intros [Hy|Hy]; [apply Na; symmetry; exact Hy | apply Ny, Hy].
      - intros y Hy. apply Pd1. right. exact Hy. }
    simpl in M. destruct (IH p1 stk1 I2) as [I3 E3]; [lia|]. split; [exact I3 | intros y t G; apply E3, E1, G].
Qed.

Definition flood_step n nb := fun p s => match get p s with
                        | Some _ => p
                        | None => drain (S n) nb s (put p s (Some s)) [s]
                        end.

Lemma flood_S n nb k : fold_left (flood_step n nb) (seq 0 (S k)) (repeat None n) =
  flood_step n nb (fold_left (flood_step n nb) (seq 0 k) (repeat None n)) k.
Proof. rewrite seq_S, fold_left_app. reflexivity. Qed.

Lemma flood_inv n nb adj (Hs : forall x y, adj x y = adj y x)
  (Hnb : forall a, (a < n)%nat -> forall b, In b (nb a) <-> (b < n)%nat /\ adj a b = true) :
  forall k, (k <= n)%nat -> Inv n adj O [] (fold_left (flood_step n nb) (seq 0 k) (repeat None n)) /\
    forall y, (y < k)%nat -> get (fold_left (flood_step n nb) (seq 0 k) (repeat None n)) y <> None.
Proof. induction k as [|k IH]; intros Hk.
  - simpl. assert (R : forall y, get (repeat None n) y = None) by (intros y; apply nth_repeat).
    split; [split; [apply repeat_length | split; [|split]] | intros y Hy; exfalso; lia].
    + intros y t G. rewrite R in G. discriminate.
    + intros y t G. rewrite R in G. discriminate.
    + intros y [].
  - rewrite flood_S. destruct (IH ltac:(lia)) as [I C]. revert I C.
    generalize (fold_left (flood_step n nb) (seq 0 k) (repeat None n)) as p. intros p I C.
    unfold flood_step. cbv beta. destruct (get p k) as [t|] eqn:G; cbv beta iota.
    + split; [exact I|]. intros y Hy. destruct (Nat.eq_dec y k) as [->|Ny]; [congruence | apply C; lia].
    + pose proof I as (Ln & Lab & Cl & _). assert (E0 : ext p (put p k (Some k))).
      { intros y t Gy. rewrite get_put_other; [exact Gy | congruence]. }
      assert (I0 : Inv n adj k [k] (put p k (Some k))).
      { split; [rewrite put_length; exact Ln | split; [|split]].
        - intros y t Gy. destruct (Nat.eq_dec y k) as [->|Ny].
          + rewrite get_put_same in Gy by lia. injection Gy as <-. split; [lia | constructor].
          + rewrite get_put_other in Gy by congruence. apply (Lab y t Gy).
        - intros y t Gy Ny z Hz Ez. rewrite get_put_other in Gy by (intros ->; apply Ny; left; reflexivity).
          apply E0, (Cl y t Gy (@in_nil _ y) z Hz Ez).
        - intros y [<-|[]]. split; [lia | apply get_put_same; lia]. }
      destruct (drain_inv n nb adj k Hs Hnb (S n) _ [k] I0) as [I1 E1].
      { cbn [length]. pose proof (nones_put p k k G ltac:(lia)).
        pose proof (filter_length_le (fun o : option nat => match o with None => true | Some _ => false end) p).
        unfold nones in *. lia. }
      split.
      * destruct I1 as (A1 & B1 & C1 & _). split; [exact A1 | split; [exact B1 | split; [exact C1 | intros y []]]].
      * intros y Hy. destruct (Nat.eq_dec y k) as [->|Ny].
        -- rewrite (E1 k k); [discriminate | apply get_put_same; lia].
        -- destruct (get p y) as [t|] eqn:Gy; [|exfalso; apply (C y); [lia | exact Gy]].
           rewrite (E1 y t (E0 y t Gy)). discriminate.
Qed.

(* The flood numbers islands: every position is reached, and two share a number exactly when one
   reaches the other, whatever order nb lists a position's neighbours in. *)
Theorem flood_parts n nb adj :
  (forall a b, adj a b = adj b a) ->
  (forall a, (a < n)%nat -> forall b, In b (nb a) <-> (b < n)%nat /\ adj a b = true) ->
  forall a b, (a < n)%nat -> (b < n)%nat ->
  get (flood n nb) a <> None /\ (get (flood n nb) a = get (flood n nb) b <-> reach n adj a b).
Proof. intros Hs Hnb a b Ha Hb. destruct (flood_inv n nb adj Hs Hnb n (le_n n)) as [(Ln & Lab & Cl & _) Cov].
  change (fold_left (flood_step n nb) (seq 0 n) (repeat None n)) with (flood n nb) in Lab, Cl, Cov.
  split; [exact (Cov a Ha)|]. destruct (get (flood n nb) a) as [t|] eqn:Ga; [|exfalso; exact (Cov a Ha Ga)]. split.
  - intros Gb. symmetry in Gb. destruct (Lab a t Ga) as [Ht Ra]. destruct (Lab b t Gb) as [_ Rb].
    apply (reach_trans n adj a t b); [apply (reach_sym n adj t a); auto | exact Rb].
  - intros R. clear Hb. induction R as [|c d R IH Hd E]; [symmetry; exact Ga|].
    symmetry. exact (Cl c t (eq_sym IH) (@in_nil _ c) d Hd E).
Qed.

(* Index::parts and place::islands: the label of each window but skip is the least id of its
   island. *)
Definition w0 := Window 0 d0 (0, 0).
Definition parts (rs : list rect) :=
  let ix := index_new rs in flood (length rs) (fun a => touching ix (nth a rs d0)).
Definition opt_eqb (a b : option nat) :=
  match a, b with Some a, Some b => Nat.eqb a b | None, None => true | _, _ => false end.
Definition islands_fast (ws : list window) (skip : list nat) : list (nat * nat) :=
  let l := filter (fun w => negb (mem (wid w) skip)) ws in
  let part := parts (map wrect l) in
  let least x := fold_left Nat.min
    (map (fun y => wid (nth y l w0)) (filter (fun y => opt_eqb (get part y) (get part x)) (seq 0 (length l))))
    (wid (nth x l w0)) in
  map (fun x => (wid (nth x l w0), least x)) (seq 0 (length l)).
Definition lookup (m : list (nat * nat)) (i : nat) := option_map snd (find (fun e => Nat.eqb (fst e) i) m).

(* ---------- connected (grow) is reachability through touch, by position ---------- *)

Lemma touch_sym a b : touch a b = touch b a.
Proof. unfold touch, rows_overlap, cols_overlap.
  rewrite (orb_comm (right b =? ri a)), (andb_comm (rj b <? bottom a)), (orb_comm (bottom b =? rj a)),
    (andb_comm (ri b <? right a)). reflexivity.
Qed.

Lemma nth_ids L k : nth k (ids L) O = wid (nth k L w0).
Proof. exact (map_nth wid L w0 k). Qed.

Lemma nth_wrect l a : nth a (map wrect l) d0 = wrect (nth a l w0).
Proof. exact (map_nth wrect l w0 a). Qed.

Lemma nth_map_lt {A B} (f : A -> B) l d d' k : (k < length l)%nat -> nth k (map f l) d' = f (nth k l d).
Proof. intros H. rewrite (nth_indep (map f l) d' (f d)) by (rewrite length_map; exact H). apply map_nth. Qed.

Lemma ids_inj L a b : NoDup (ids L) -> (a < length L)%nat -> (b < length L)%nat ->
  wid (nth a L w0) = wid (nth b L w0) -> a = b.
Proof. intros Nd Ha Hb E. apply (proj1 (NoDup_nth (ids L) O) Nd);
    [unfold ids; rewrite length_map; exact Ha | unfold ids; rewrite length_map; exact Hb | rewrite !nth_ids; exact E].
Qed.

Lemma in_ids_nth L i : In i (ids L) -> exists x, (x < length L)%nat /\ wid (nth x L w0) = i.
Proof. intros H. apply (In_nth _ _ O) in H as (x & Hx & E). unfold ids in Hx. rewrite length_map in Hx.
  exists x. split; [exact Hx | rewrite <- nth_ids; exact E].
Qed.

Lemma nth_in_ids L k : (k < length L)%nat -> In (wid (nth k L w0)) (ids L).
Proof. intros H. rewrite <- nth_ids. apply nth_In. unfold ids. rewrite length_map. exact H. Qed.

Lemma filter_count_le {A} (f g : A -> bool) l : (forall y, In y l -> f y = true -> g y = true) ->
  (length (filter f l) <= length (filter g l))%nat.
Proof. induction l as [|y l IH]; simpl; intros H; [lia|]. specialize (IH (fun z Hz => H z (or_intror Hz))).
  destruct (f y) eqn:F; [rewrite (H y (or_introl eq_refl) F)|destruct (g y)]; simpl; lia.
Qed.

Lemma filter_count_lt {A} (f g : A -> bool) l x : (forall y, In y l -> f y = true -> g y = true) ->
  In x l -> g x = true -> f x = false -> (length (filter f l) < length (filter g l))%nat.
Proof. induction l as [|y l IH]; simpl; intros H Hx Gx Fx; [contradiction|].
  pose proof (filter_count_le f g l (fun z Hz => H z (or_intror Hz))) as Le.
  destruct Hx as [<-|Hx]; [rewrite Gx, Fx; simpl; lia|]. specialize (IH (fun z Hz => H z (or_intror Hz)) Hx Gx Fx).
  destruct (f y) eqn:F; [rewrite (H y (or_introl eq_refl) F)|destruct (g y)]; simpl; lia.
Qed.

Definition gstep ws (seen : list nat) := filter (fun w => negb (mem (wid w) seen) &&
  existsb (fun v => mem (wid v) seen && touch (wrect v) (wrect w)) ws) ws.
Definition closed_ids ws (seen : list nat) :=
  forall v w, In v ws -> In w ws -> In (wid v) seen -> touch (wrect v) (wrect w) = true -> In (wid w) seen.
Definition cnt ws (seen : list nat) := length (filter (fun w => mem (wid w) seen) ws).

Lemma grow_S n ws seen : grow (S n) ws seen = grow n ws (seen ++ map wid (gstep ws seen)).
Proof. reflexivity. Qed.

Lemma grow_mono n ws seen i : In i seen -> In i (grow n ws seen).
Proof. revert seen; induction n as [|n IH]; intros seen H; [exact H|]. rewrite grow_S. apply IH, in_or_app; left; exact H. Qed.

Lemma gstep_closed ws seen : gstep ws seen = [] -> closed_ids ws seen.
Proof. intros E v w Hv Hw Iv T. destruct (mem (wid w) seen) eqn:M; [apply mem_iff, M|]. exfalso.
  assert (H : In w (gstep ws seen)); [|rewrite E in H; exact H].
  apply filter_In. split; [exact Hw|]. rewrite M. simpl. apply existsb_exists. exists v.
  split; [exact Hv|]. rewrite (proj2 (mem_iff _ _) Iv). exact T.
Qed.

Lemma closed_fix n ws seen : gstep ws seen = [] -> grow n ws seen = seen.
Proof. intros E. induction n as [|n IH]; [reflexivity|]. rewrite grow_S, E. cbn [map]. rewrite app_nil_r. exact IH. Qed.

(* Each round either adds a window or finds the set closed. *)
Lemma grow_closed ws : forall n seen, closed_ids ws (grow n ws seen) \/ (n + cnt ws seen <= cnt ws (grow n ws seen))%nat.
Proof. induction n as [|n IH]; intros seen; [right; simpl; lia|].
  rewrite grow_S. destruct (gstep ws seen) as [|w l] eqn:E.
  - left. cbn [map]. rewrite app_nil_r, (closed_fix n ws seen E). apply gstep_closed, E.
  - destruct (IH (seen ++ map wid (w :: l))) as [H|H]; [left; exact H|right].
    assert (Hw : In w (gstep ws seen)) by (rewrite E; left; reflexivity).
    unfold gstep in Hw. apply filter_In in Hw as [Hw P]. apply andb_true_iff in P as [P _]. apply negb_true_iff in P.
    assert (cnt ws seen < cnt ws (seen ++ map wid (w :: l)))%nat; [|lia].
    unfold cnt. apply (filter_count_lt _ _ _ w).
    + intros y _ M. cbv beta in *. apply mem_iff in M. apply mem_iff, in_or_app. left. exact M.
    + exact Hw.
    + apply mem_iff, in_or_app. right. left. reflexivity.
    + exact P.
Qed.

Lemma grow_full L seen : closed_ids L (grow (length L) L seen).
Proof. destruct (grow_closed L (length L) seen) as [H|H]; [exact H|]. intros v w _ Hw _ _.
  assert (Lf : length (filter (fun x => mem (wid x) (grow (length L) L seen)) L) = length L).
  { pose proof (filter_length_le (fun x => mem (wid x) (grow (length L) L seen)) L). unfold cnt in H. lia. }
  apply filter_length_forallb in Lf. apply mem_iff. exact (proj1 (forallb_forall _ _) Lf w Hw).
Qed.

Lemma connected_reach L adj x y : NoDup (ids L) ->
  (forall a b, adj a b = touch (wrect (nth a L w0)) (wrect (nth b L w0))) ->
  (x < length L)%nat -> (y < length L)%nat ->
  connected L (wid (nth x L w0)) (wid (nth y L w0)) = true <-> reach (length L) adj x y.
Proof. intros Nd Hadj Hx Hy. unfold connected. rewrite mem_iff. split.
  - assert (Snd : forall k seen,
      (forall i, In i seen -> exists z, (z < length L)%nat /\ wid (nth z L w0) = i /\ reach (length L) adj x z) ->
      forall i, In i (grow k L seen) -> exists z, (z < length L)%nat /\ wid (nth z L w0) = i /\ reach (length L) adj x z).
    { induction k as [|k IH]; intros seen Sd; [exact Sd|]. rewrite grow_S. apply IH. intros i Hi.
      apply in_app_or in Hi as [Hi|Hi]; [apply Sd, Hi|].
      apply in_map_iff in Hi as (w & <- & Hw). unfold gstep in Hw. apply filter_In in Hw as [Hw P].
      apply andb_true_iff in P as [_ P]. apply existsb_exists in P as (v & Hv & P). apply andb_true_iff in P as [M T].
      apply mem_iff, Sd in M as (z & Hz & Ez & Rz).
      apply (In_nth L w w0) in Hw as (zw & Hzw & <-). apply (In_nth L v w0) in Hv as (zv & Hzv & <-).
      assert (zv = z) as -> by (apply (ids_inj L); auto; congruence).
      exists zw. split; [exact Hzw | split; [reflexivity | apply (reach_step _ _ _ z); auto]].
      rewrite Hadj. exact T. }
    intros H. assert (S0 : forall i, In i [wid (nth x L w0)] ->
      exists z, (z < length L)%nat /\ wid (nth z L w0) = i /\ reach (length L) adj x z).
    { intros i [<-|[]]. exists x. split; [exact Hx | split; [reflexivity | constructor]]. }
    destruct (Snd _ _ S0 _ H) as (z & Hz & Ez & Rz). assert (z = y) as -> by (apply (ids_inj L); auto). exact Rz.
  - intros R. clear Hy. induction R as [|c d R IH Hd E].
    + apply grow_mono. left. reflexivity.
    + apply (grow_full L [wid (nth x L w0)] (nth c L w0) (nth d L w0)).
      * apply nth_In. apply (reach_lt (length L) adj x c); auto.
      * apply nth_In, Hd.
      * exact IH.
      * rewrite <- Hadj. exact E.
Qed.

Lemma connected_refl L a : connected L a a = true.
Proof. unfold connected. apply mem_iff, grow_mono. left; reflexivity. Qed.

Lemma connected_sym L a b : NoDup (ids L) -> In a (ids L) -> In b (ids L) -> connected L a b = connected L b a.
Proof. intros Nd (x & Hx & <-)%in_ids_nth (y & Hy & <-)%in_ids_nth. apply eq_true_iff_eq.
  rewrite !(connected_reach L (fun a b => touch (wrect (nth a L w0)) (wrect (nth b L w0)))) by auto.
  split; apply reach_sym; auto; intros; cbv beta; apply touch_sym.
Qed.

Lemma ids_but_in skip ws i : In i (ids (but skip ws)) -> In i (ids ws).
Proof. unfold ids, but. intros (w & <- & H)%in_map_iff. apply filter_In in H as [H _]. apply in_map, H. Qed.

Lemma nodup_but skip ws : NoDup (ids ws) -> NoDup (ids (but skip ws)).
Proof. unfold ids, but. induction ws as [|w ws IH]; simpl; intros Nd; [constructor|].
  inversion Nd as [|? ? Nw Nd']; subst. destruct (negb (mem (wid w) skip)); simpl; [constructor|]; auto.
  intros H; apply Nw. apply in_map_iff in H as (v & E & Hv). apply filter_In in Hv as [Hv _].
  rewrite <- E. apply in_map, Hv.
Qed.

Lemma but_nil ws : but [] ws = ws.
Proof. unfold but. rewrite (filter_ext _ (fun _ => true)) by (intros; reflexivity).
  induction ws as [|w ws IH]; simpl; congruence.
Qed.

(* ---------- islands_fast labels each window of an island with one number ---------- *)

Lemma opt_eqb_iff a b : opt_eqb a b = true <-> a = b.
Proof. destruct a as [a|], b as [b|]; simpl; rewrite ?Nat.eqb_eq; split; intros; congruence. Qed.

Lemma some_eq_iff (a b : nat) : Some a = Some b <-> a = b.
Proof. split; intros; congruence. Qed.

Definition least_of (l : list window) (part : list (option nat)) (x : nat) :=
  fold_left Nat.min (map (fun y => wid (nth y l w0)) (filter (fun y => opt_eqb (get part y) (get part x))
    (seq 0 (length l)))) (wid (nth x l w0)).

Lemma islands_fast_eq ws skip : islands_fast ws skip =
  map (fun x => (wid (nth x (but skip ws) w0), least_of (but skip ws) (parts (map wrect (but skip ws))) x))
    (seq 0 (length (but skip ws))).
Proof. reflexivity. Qed.

Lemma lookup_at l part x : NoDup (ids l) -> (x < length l)%nat ->
  lookup (map (fun x => (wid (nth x l w0), least_of l part x)) (seq 0 (length l))) (wid (nth x l w0)) =
  Some (least_of l part x).
Proof. intros Nd Hx. unfold lookup. destruct (find _ _) as [[i v]|] eqn:F.
  - apply find_some in F as [F E]. apply in_map_iff in F as (z & Ez & Hz). apply in_seq in Hz.
    injection Ez as <- <-. simpl in E. apply Nat.eqb_eq in E.
    assert (z = x) as -> by (apply (ids_inj l); auto; lia). reflexivity.
  - exfalso. assert (H : In (wid (nth x l w0), least_of l part x)
      (map (fun x => (wid (nth x l w0), least_of l part x)) (seq 0 (length l)))).
    { apply in_map_iff. exists x. split; [reflexivity | apply in_seq; lia]. }
    apply (find_none _ _ F) in H. simpl in H. rewrite Nat.eqb_refl in H. discriminate.
Qed.

Lemma lookup_none l part i : ~ In i (ids l) ->
  lookup (map (fun x => (wid (nth x l w0), least_of l part x)) (seq 0 (length l))) i = None.
Proof. intros N. unfold lookup. destruct (find _ _) as [[j v]|] eqn:F; [|reflexivity]. exfalso.
  apply find_some in F as [F E]. apply in_map_iff in F as (z & Ez & Hz). apply in_seq in Hz.
  injection Ez as <- <-. simpl in E. apply Nat.eqb_eq in E. apply N. rewrite <- E. apply nth_in_ids. lia.
Qed.

Lemma fold_min_spec (L : list nat) i : (fold_left Nat.min L i <= i)%nat /\
  (forall z, In z L -> (fold_left Nat.min L i <= z)%nat) /\ (fold_left Nat.min L i = i \/ In (fold_left Nat.min L i) L).
Proof. revert i; induction L as [|a L IH]; intros i; simpl; [split; [lia | split; [tauto | auto]]|].
  destruct (IH (Nat.min i a)) as (H1 & H2 & H3). split; [lia | split].
  - intros z [<-|Hz]; [lia | apply H2, Hz].
  - destruct H3 as [H3|H3]; [|right; right; exact H3]. rewrite H3.
    destruct (Nat.min_spec i a) as [[_ ->]|[_ ->]]; [left; reflexivity | right; left; reflexivity].
Qed.

Lemma least_in l part z : (z < length l)%nat ->
  let F := filter (fun y => opt_eqb (get part y) (get part z)) (seq 0 (length l)) in
  (exists u, In u F /\ least_of l part z = wid (nth u l w0)) /\
  (forall u, In u F -> (least_of l part z <= wid (nth u l w0))%nat).
Proof. intros Hz F. unfold least_of; fold F.
  destruct (fold_min_spec (map (fun y => wid (nth y l w0)) F) (wid (nth z l w0))) as (H1 & H2 & H3).
  assert (Fz : In z F) by (apply filter_In; split; [apply in_seq; lia | apply opt_eqb_iff; reflexivity]). split.
  - destruct H3 as [H3|H3]; [exists z; split; assumption|].
    apply in_map_iff in H3 as (u & E & Hu). exists u. split; [exact Hu | symmetry; exact E].
  - intros u Hu. apply H2, (in_map (fun y => wid (nth y l w0))), Hu.
Qed.

Lemma least_eq l part x y : NoDup (ids l) -> (x < length l)%nat -> (y < length l)%nat ->
  least_of l part x = least_of l part y <-> get part x = get part y.
Proof. intros Nd Hx Hy. destruct (least_in l part x Hx) as [(u & Hu & Eu) Lx].
  destruct (least_in l part y Hy) as [(v & Hv & Ev) Ly].
  apply filter_In in Hu as [Hu Gu]. apply filter_In in Hv as [Hv Gv].
  apply in_seq in Hu, Hv. apply opt_eqb_iff in Gu, Gv. split; intros E.
  - assert (u = v) as <- by (apply (ids_inj l); auto; lia). congruence.
  - assert (A : (least_of l part x <= wid (nth v l w0))%nat).
    { apply Lx, filter_In. split; [apply in_seq; lia | apply opt_eqb_iff; congruence]. }
    assert (B : (least_of l part y <= wid (nth u l w0))%nat).
    { apply Ly, filter_In. split; [apply in_seq; lia | apply opt_eqb_iff; congruence]. } lia.
Qed.

Lemma parts_spec rs : Forall (fun r => 0 <= rw r /\ 0 <= rh r) rs ->
  forall a b, (a < length rs)%nat -> (b < length rs)%nat ->
  get (parts rs) a <> None /\
  (get (parts rs) a = get (parts rs) b <-> reach (length rs) (fun a b => touch (nth a rs d0) (nth b rs d0)) a b).
Proof. intros F. apply (flood_parts (length rs) (fun a => touching (index_new rs) (nth a rs d0)));
    [intros; apply touch_sym|].
  intros a Ha b.
  assert (Na : 0 <= rw (nth a rs d0) /\ 0 <= rh (nth a rs d0)) by (rewrite Forall_forall in F; apply F, nth_In, Ha).
  rewrite (touching_scan rs (nth a rs d0) F (proj1 Na) (proj2 Na)), filter_In, in_seq.
  split; intros [H1 H2]; split; auto; lia.
Qed.

Lemma islands_spec ws skip : NoDup (ids ws) -> spans_ok ws ->
  (forall i, lookup (islands_fast ws skip) i <> None <-> In i (ids (but skip ws))) /\
  (forall i j, In i (ids (but skip ws)) -> In j (ids (but skip ws)) ->
     (lookup (islands_fast ws skip) i = lookup (islands_fast ws skip) j <-> connected (but skip ws) i j = true)).
Proof. intros Nd Sp. rewrite islands_fast_eq. set (l := but skip ws).
  assert (Nl : NoDup (ids l)) by (apply nodup_but, Nd).
  assert (Fl : Forall (fun r => 0 <= rw r /\ 0 <= rh r) (map wrect l)).
  { apply Forall_forall. intros r (w & <- & Hw)%in_map_iff. apply Sp. unfold l, but in Hw. apply filter_In in Hw. tauto. }
  pose proof (parts_spec _ Fl) as P. rewrite length_map in P. split.
  - intros i. split.
    + intros H. destruct (in_dec Nat.eq_dec i (ids l)) as [Hi|Hi]; [exact Hi|]. exfalso. apply H, lookup_none, Hi.
    + intros (x & Hx & <-)%in_ids_nth. rewrite lookup_at by auto. discriminate.
  - intros i j (x & Hx & <-)%in_ids_nth (y & Hy & <-)%in_ids_nth.
    rewrite !lookup_at by auto. rewrite some_eq_iff, least_eq by auto.
    rewrite (proj2 (P x y Hx Hy)). symmetry. apply connected_reach; auto.
    intros a b. cbv beta. rewrite !nth_wrect. reflexivity.
Qed.

Lemma existsb_ext_in {A} (f g : A -> bool) l : (forall x, In x l -> f x = g x) -> existsb f l = existsb g l.
Proof. induction l as [|x l IH]; simpl; intros H; [reflexivity|].
  rewrite (H x (or_introl eq_refl)), IH; [reflexivity|]. intros y Hy; apply H; right; exact Hy.
Qed.

(* place::split_by: one pass over the windows before, remembering for each island before the
   island after of its first window (entry().or_insert), true at a window found on another. *)
Fixpoint split_pass (b a : nat -> option nat) (first : list (nat * nat)) (ws : list window) : bool :=
  match ws with
  | [] => false
  | w :: ws' =>
      match b (wid w), a (wid w) with
      | Some lb, Some la =>
          match lookup first lb with
          | Some l => if Nat.eqb l la then split_pass b a first ws' else true
          | None => split_pass b a ((lb, la) :: first) ws'
          end
      | _, _ => split_pass b a first ws'
      end
  end.
Definition splits_fast ws ws' movers :=
  split_pass (lookup (islands_fast ws movers)) (lookup (islands_fast ws' [])) [] ws.

(* ---------- A pass over (key, value) entries that remembers each key's first value ---------- *)

Fixpoint gpass {V} (eqv : V -> V -> bool) (first : list (nat * V)) (l : list (option (nat * V))) : bool :=
  match l with
  | [] => false
  | None :: l' => gpass eqv first l'
  | Some (k, v) :: l' =>
      match find (fun e => Nat.eqb (fst e) k) first with
      | Some (_, v0) => if eqv v0 v then gpass eqv first l' else true
      | None => gpass eqv ((k, v) :: first) l'
      end
  end.

Definition conflict {V} (P : nat * V -> Prop) := exists x y, P x /\ P y /\ fst x = fst y /\ snd x <> snd y.

Lemma conflict_iff {V} (P Q : nat * V -> Prop) : (forall x, P x <-> Q x) -> conflict P <-> conflict Q.
Proof. intros H; unfold conflict; split; intros (x & y & Px & Py & E & N); exists x, y;
    (split; [apply H; exact Px | split; [apply H; exact Py | split; [exact E | exact N]]]).
Qed.

(* The pass answers true exactly when two entries share a key and differ in value. *)
Lemma gpass_conflict {V} (eqv : V -> V -> bool) : (forall x y, eqv x y = true <-> x = y) ->
  forall l first, (forall e1 e2, In e1 first -> In e2 first -> fst e1 = fst e2 -> snd e1 = snd e2) ->
  gpass eqv first l = true <-> conflict (fun e => In e first \/ In (Some e) l).
Proof. intros Heq. induction l as [|[[k v]|] l IH]; intros first Hf; simpl.
  - split; [discriminate|]. intros (x & y & [Hx|[]] & [Hy|[]] & E & N). exfalso. apply N, Hf; auto.
  - destruct (find (fun e => Nat.eqb (fst e) k) first) as [[k0 v0]|] eqn:F.
    + apply find_some in F as [F E0]. simpl in E0. apply Nat.eqb_eq in E0. subst k0. destruct (eqv v0 v) eqn:Ev.
      * apply Heq in Ev. subst v0. rewrite IH by auto. apply conflict_iff. intros x. simpl.
        split; [intros [H|H]; auto | intros [H|[H|H]]; auto; injection H as <-; left; exact F].
      * split; [intros _ | reflexivity]. exists (k, v0), (k, v). simpl.
        split; [left; exact F | split; [right; left; reflexivity | split; [reflexivity |]]].
        intros ->. rewrite (proj2 (Heq v v) eq_refl) in Ev. discriminate.
    + rewrite IH.
      * apply conflict_iff. intros x. simpl.
        split; [intros [[H|H]|H]; [subst x; auto | auto | auto] | intros [H|[H|H]]; [auto | injection H as <-; auto | auto]].
      * intros e1 e2 [<-|H1] [<-|H2] E; simpl in *; auto.
        -- pose proof (find_none _ _ F e2 H2) as N. simpl in N. rewrite <- E, Nat.eqb_refl in N. discriminate.
        -- pose proof (find_none _ _ F e1 H1) as N. simpl in N. rewrite E, Nat.eqb_refl in N. discriminate.
  - rewrite IH by auto. apply conflict_iff. intros x. simpl.
    split; [intros [H|H]; auto | intros [H|[H|H]]; [left; exact H | discriminate H | right; exact H]].
Qed.

Lemma split_pass_gpass (B A : nat -> option nat) first ws :
  split_pass B A first ws = gpass Nat.eqb first (map (fun w => match B (wid w), A (wid w) with
    | Some lb, Some la => Some (lb, la) | _, _ => None end) ws).
Proof. revert first; induction ws as [|w ws IH]; intros first; simpl; [reflexivity|].
  destruct (B (wid w)) as [lb|]; simpl; [|apply IH]. destruct (A (wid w)) as [la|]; simpl; [|apply IH].
  unfold lookup. destruct (find _ first) as [[k0 v0]|]; simpl; [destruct (Nat.eqb v0 la)|]; auto.
Qed.

(* Both checks answer whether two windows of one island before (and in the layout after) lie on
   two islands after. *)
Definition split_gap (B : nat -> option nat) ws ws' := exists a b, In a (ids ws) /\ In b (ids ws) /\
  B a <> None /\ B a = B b /\ In a (ids ws') /\ In b (ids ws') /\ connected ws' a b = false.

Lemma pass_gap ws ws' (B A : nat -> option nat) :
  (forall i, A i <> None <-> In i (ids ws')) ->
  (forall i j, In i (ids ws') -> In j (ids ws') -> (A i = A j <-> connected ws' i j = true)) ->
  split_pass B A [] ws = true <-> split_gap B ws ws'.
Proof. intros HA HAc. rewrite split_pass_gpass, gpass_conflict; [|apply Nat.eqb_eq | intros ? ? []].
  unfold conflict, split_gap. split.
  - intros (x & y & [[]|Hx] & [[]|Hy] & Ef & Ns).
    apply in_map_iff in Hx as (u & Eu & Hu). apply in_map_iff in Hy as (w & Ew & Hw).
    destruct (B (wid u)) as [lu|] eqn:Bu; [|discriminate Eu]. destruct (A (wid u)) as [au|] eqn:Au; [|discriminate Eu].
    destruct (B (wid w)) as [lw|] eqn:Bw; [|discriminate Ew]. destruct (A (wid w)) as [aw|] eqn:Aw; [|discriminate Ew].
    injection Eu as <-. injection Ew as <-. simpl in Ef, Ns.
    assert (Iu : In (wid u) (ids ws')) by (apply HA; rewrite Au; discriminate).
    assert (Iw : In (wid w) (ids ws')) by (apply HA; rewrite Aw; discriminate). exists (wid u), (wid w).
    split; [apply in_map, Hu|]. split; [apply in_map, Hw|]. split; [rewrite Bu; discriminate|].
    split; [rewrite Bu, Bw, Ef; reflexivity|]. split; [exact Iu|]. split; [exact Iw|].
    destruct (connected ws' (wid u) (wid w)) eqn:C; [|reflexivity]. exfalso. apply Ns.
    apply (HAc _ _ Iu Iw) in C. rewrite Au, Aw in C. injection C as C. exact C.
  - intros (a & b & Ha & Hb & Na & E & Ha' & Hb' & C). destruct (B a) as [la|] eqn:Ba; [|congruence].
    destruct (A a) as [aa|] eqn:Aa; [|exfalso; apply (proj2 (HA a) Ha'); exact Aa].
    destruct (A b) as [ab|] eqn:Ab; [|exfalso; apply (proj2 (HA b) Hb'); exact Ab].
    apply in_map_iff in Ha as (u & Eu & Hu). apply in_map_iff in Hb as (w & Ew & Hw). exists (la, aa), (la, ab).
    split; [right; apply in_map_iff; exists u; rewrite Eu, Ba, Aa; split; [reflexivity | exact Hu]|].
    split; [right; apply in_map_iff; exists w; rewrite Ew, <- E, Ab; split; [reflexivity | exact Hw]|].
    split; [reflexivity|]. simpl. intros <-.
    assert (G : A a = A b) by congruence. apply (HAc a b Ha' Hb') in G. congruence.
Qed.

Lemma splits_gap ws ws' movers (B : nat -> option nat) : NoDup (ids ws') ->
  (forall i, B i <> None <-> In i (ids (but movers ws))) ->
  (forall i j, In i (ids (but movers ws)) -> In j (ids (but movers ws)) ->
     (B i = B j <-> connected (but movers ws) i j = true)) ->
  splits ws ws' movers = true <-> split_gap B ws ws'.
Proof. intros Nd' HB HBc. unfold splits, split_gap. rewrite existsb_exists. split.
  - intros (a & Ha & H). apply existsb_exists in H as (b & Hb & H).
    apply andb_true_iff in H as [H Hc']. apply andb_true_iff in H as [H Mb].
    apply andb_true_iff in H as [H Ma]. apply andb_true_iff in H as [_ Hc].
    exists a, b. split; [apply (ids_but_in movers), Ha|]. split; [apply (ids_but_in movers), Hb|].
    split; [apply HB, Ha|]. split; [apply HBc; assumption|].
    split; [apply mem_iff, Ma|]. split; [apply mem_iff, Mb|]. apply negb_true_iff, Hc'.
  - intros (a & b & _ & _ & Na & E & Ha' & Hb' & C).
    assert (Ia : In a (ids (but movers ws))) by (apply HB; exact Na).
    assert (Ib : In b (ids (but movers ws))) by (apply HB; congruence). destruct (Nat.lt_total a b) as [L|[<-|L]].
    + exists a. split; [exact Ia|]. apply existsb_exists. exists b. split; [exact Ib|].
      rewrite (proj2 (Nat.ltb_lt _ _) L), (proj1 (HBc a b Ia Ib) E), (proj2 (mem_iff _ _) Ha'),
        (proj2 (mem_iff _ _) Hb'), C. reflexivity.
    + rewrite connected_refl in C. discriminate.
    + exists b. split; [exact Ib|]. apply existsb_exists. exists a. split; [exact Ia|].
      rewrite (proj2 (Nat.ltb_lt _ _) L), (proj1 (HBc b a Ib Ia) (eq_sym E)), (proj2 (mem_iff _ _) Ha'),
        (proj2 (mem_iff _ _) Hb'), (connected_sym ws' b a Nd' Hb' Ha'), C. reflexivity.
Qed.

Theorem splits_fast_spec ws ws' movers :
  NoDup (ids ws) -> NoDup (ids ws') -> spans_ok ws -> spans_ok ws' ->
  splits_fast ws ws' movers = splits ws ws' movers.
Proof. intros Nd Nd' Sp Sp'. destruct (islands_spec ws movers Nd Sp) as [HB HBc].
  destruct (islands_spec ws' [] Nd' Sp') as [HA HAc]. rewrite but_nil in HA, HAc.
  apply eq_true_iff_eq. unfold splits_fast.
  rewrite (pass_gap ws ws' _ _ HA HAc). symmetry. exact (splits_gap ws ws' movers _ Nd' HB HBc).
Qed.

(* place::is_bridge over islands_fast. *)
Definition is_bridge_fast ws i :=
  match window_of i ws with
  | None => false
  | Some w =>
      let near := ids (filter (fun o => negb (Nat.eqb (wid o) i) && touch (wrect o) (wrect w)) ws) in
      let lab := lookup (islands_fast ws [i]) in
      existsb (fun a => existsb (fun b => negb (opt_eqb (lab a) (lab b))) near) near
  end.

Theorem is_bridge_fast_spec ws i : NoDup (ids ws) -> spans_ok ws -> is_bridge_fast ws i = is_bridge ws i.
Proof. intros Nd Sp. unfold is_bridge_fast, is_bridge. destruct (window_of i ws) as [w|]; [|reflexivity]. cbv zeta.
  destruct (islands_spec ws [i] Nd Sp) as [_ Hc].
  assert (Near : forall a, In a (ids (filter (fun o => negb (Nat.eqb (wid o) i) && touch (wrect o) (wrect w)) ws)) ->
    In a (ids (but [i] ws))).
  { unfold ids, but. intros a (o & <- & Ho)%in_map_iff. apply filter_In in Ho as [Ho P].
    apply andb_true_iff in P as [P _]. apply in_map, filter_In. split; [exact Ho|].
    unfold mem. simpl. rewrite orb_false_r. exact P. }
  apply existsb_ext_in. intros a Ha. cbv beta. apply existsb_ext_in. intros b Hb. cbv beta. f_equal.
  apply eq_true_iff_eq. rewrite opt_eqb_iff. apply Hc; apply Near; assumption.
Qed.

(* place::Search::splits, at a layout cur the search reaches from the windows ws: the flood finds
   the windows still where they were in the range tree over the layout before (touching, then
   kept when unmoved) and the moved ones by a scan; island numbers each window's island before
   without the movers in order of first appearance (None for a mover), and one pass looks for an
   island before whose windows lie on two after. *)
Definition leaf_nb (ws : list window) (cur : list rect) (a : nat) :=
  let before := map wrect ws in
  let moved := filter (fun k => negb (rect_eqb (nth k cur d0) (nth k before d0))) (seq 0 (length cur)) in
  filter (fun b => rect_eqb (nth b cur d0) (nth b before d0)) (touching (index_new before) (nth a cur d0)) ++
  filter (fun b => touch (nth a cur d0) (nth b cur d0)) moved.

Fixpoint number (labels : list (option nat)) (seen : list nat) : list (option nat) :=
  match labels with
  | [] => []
  | None :: rest => None :: number rest seen
  | Some l :: rest =>
      match find (fun e => Nat.eqb (fst e) l) (combine seen (seq 0 (length seen))) with
      | Some (_, i) => Some i :: number rest seen
      | None => Some (length seen) :: number rest (seen ++ [l])
      end
  end.
Definition island ws movers := number (map (fun w => lookup (islands_fast ws movers) (wid w)) ws) [].

Fixpoint leaf_pass (island : list (option nat)) (part : list (option nat)) (first : list (nat * option nat))
    (x : nat) : bool :=
  match island with
  | [] => false
  | None :: rest => leaf_pass rest part first (S x)
  | Some i :: rest =>
      match find (fun e => Nat.eqb (fst e) i) first with
      | None => leaf_pass rest part ((i, get part x) :: first) (S x)
      | Some (_, p) => if opt_eqb p (get part x) then leaf_pass rest part first (S x) else true
      end
  end.
Definition leaf_splits ws movers (cur : list rect) :=
  leaf_pass (island ws movers) (flood (length cur) (leaf_nb ws cur)) [] O.

(* ---------- leaf_pass is the same pass by position; number renames labels one to one ---------- *)

Fixpoint leaf_entries (isl part : list (option nat)) (x : nat) : list (option (nat * option nat)) :=
  match isl with
  | [] => []
  | None :: r => None :: leaf_entries r part (S x)
  | Some i :: r => Some (i, get part x) :: leaf_entries r part (S x)
  end.

Lemma leaf_pass_gpass isl part first x : leaf_pass isl part first x = gpass opt_eqb first (leaf_entries isl part x).
Proof. revert first x; induction isl as [|[i|] isl IH]; intros first x; simpl; auto.
  destruct (find _ first) as [[k0 v0]|]; [destruct (opt_eqb v0 (get part x))|]; auto.
Qed.

Lemma leaf_entries_in isl part x e : In (Some e) (leaf_entries isl part x) <->
  exists k, (k < length isl)%nat /\ nth k isl None = Some (fst e) /\ snd e = get part (x + k)%nat.
Proof. revert x; induction isl as [|o isl IH]; intros x; simpl.
  - split; [tauto | intros (k & H & _); lia].
  - destruct o as [i|]; simpl; rewrite IH; split.
    + intros [H|(k & Hk & Hn & Hs)].
      * injection H as <-. exists O. simpl. rewrite Nat.add_0_r. split; [lia | split; reflexivity].
      * exists (S k). simpl. split; [lia | split; [exact Hn | rewrite Hs; f_equal; lia]].
    + intros ([|k] & Hk & Hn & Hs); simpl in Hn.
      * left. destruct e as [i' v]. simpl in *. injection Hn as <-. rewrite Nat.add_0_r in Hs. subst v. reflexivity.
      * right. exists k. split; [lia | split; [exact Hn | rewrite Hs; f_equal; lia]].
    + intros [H|(k & Hk & Hn & Hs)]; [discriminate H|].
      exists (S k). simpl. split; [lia | split; [exact Hn | rewrite Hs; f_equal; lia]].
    + intros ([|k] & Hk & Hn & Hs); simpl in Hn; [discriminate Hn|].
      right. exists k. split; [lia | split; [exact Hn | rewrite Hs; f_equal; lia]].
Qed.

Fixpoint posk (L : list nat) (l : nat) : option nat :=
  match L with [] => None | a :: L' => if Nat.eqb a l then Some O else option_map S (posk L' l) end.

Lemma find_posk L l k : find (fun e => Nat.eqb (fst e) l) (combine L (seq k (length L))) =
  option_map (fun i => (l, (k + i)%nat)) (posk L l).
Proof. revert k; induction L as [|a L IH]; intros k; simpl; [reflexivity|]. destruct (Nat.eqb a l) eqn:E; simpl.
  - apply Nat.eqb_eq in E; subst. rewrite Nat.add_0_r. reflexivity.
  - rewrite IH. destruct (posk L l); simpl; [do 2 f_equal; lia | reflexivity].
Qed.

Lemma posk_none L l : posk L l = None -> ~ In l L.
Proof. induction L as [|a L IH]; simpl; [auto|]. destruct (Nat.eqb a l) eqn:E; [intros H; discriminate H|].
  destruct (posk L l) eqn:P; [intros H; discriminate H|].
  intros _ [<-|H]; [rewrite Nat.eqb_refl in E; discriminate E | exact (IH eq_refl H)].
Qed.

Lemma posk_in L l : In l L -> exists i, posk L l = Some i.
Proof. intros H. destruct (posk L l) as [i|] eqn:E; [eauto | exfalso; exact (posk_none L l E H)]. Qed.

Lemma posk_some_in L l i : posk L l = Some i -> In l L.
Proof. revert i; induction L as [|a L IH]; simpl; intros i; [intros H; discriminate H|].
  destruct (Nat.eqb a l) eqn:E; [intros _; left; apply Nat.eqb_eq, E|].
  destruct (posk L l) as [j|] eqn:P; [intros _; right; exact (IH j eq_refl) | intros H; discriminate H].
Qed.

Lemma posk_app L M l i : posk L l = Some i -> posk (L ++ M) l = Some i.
Proof. revert i; induction L as [|a L IH]; simpl; intros i; [intros H; discriminate H|].
  destruct (Nat.eqb a l); [auto|]. destruct (posk L l) as [j|] eqn:P; simpl; [|intros H; discriminate H].
  rewrite (IH j eq_refl). simpl. auto.
Qed.

Lemma posk_last L l : posk L l = None -> posk (L ++ [l]) l = Some (length L).
Proof. induction L as [|a L IH]; simpl; [rewrite Nat.eqb_refl; auto|].
  destruct (Nat.eqb a l); [intros H; discriminate H|]. destruct (posk L l) eqn:P; [intros H; discriminate H|].
  intros _. rewrite (IH eq_refl). reflexivity.
Qed.

Lemma posk_nth L l i : posk L l = Some i -> nth i L O = l.
Proof. revert i; induction L as [|a L IH]; simpl; intros i; [intros H; discriminate H|].
  destruct (Nat.eqb a l) eqn:E; [intros H; injection H as <-; apply Nat.eqb_eq, E|].
  destruct (posk L l) as [j|] eqn:P; simpl; [|intros H; discriminate H].
  intros H. injection H as <-. apply (IH j eq_refl).
Qed.

Lemma number_map labels seen : exists F, (exists ext, F = seen ++ ext) /\ (forall l, In (Some l) labels -> In l F) /\
  number labels seen = map (option_map (fun l => match posk F l with Some i => i | None => O end)) labels.
Proof. revert seen; induction labels as [|[l|] r IH]; intros seen; simpl.
  - exists seen. split; [exists []; rewrite app_nil_r; reflexivity | split; [tauto | reflexivity]].
  - rewrite find_posk. destruct (posk seen l) as [i|] eqn:P; simpl.
    + destruct (IH seen) as (F & (ext & ->) & Hin & ->). exists (seen ++ ext). split; [eauto | split].
      * intros l' [H|H]; [injection H as <- | auto]. apply in_or_app; left. exact (posk_some_in _ _ _ P).
      * rewrite (posk_app _ _ _ _ P). reflexivity.
    + destruct (IH (seen ++ [l])) as (F & (ext & ->) & Hin & ->). exists ((seen ++ [l]) ++ ext).
      split; [exists (l :: ext); rewrite <- app_assoc; reflexivity | split].
      * intros l' [H|H]; [injection H as <- | auto]. apply in_or_app; left; apply in_or_app; right; left; reflexivity.
      * rewrite (posk_app _ _ _ _ (posk_last _ _ P)). reflexivity.
  - destruct (IH seen) as (F & HF & Hin & ->). exists F.
    split; [exact HF | split; [intros l' [H|H]; [discriminate H | auto] | reflexivity]].
Qed.

Lemma relayout_nth ws cur k : length cur = length ws -> nth k (relayout ws cur) w0 = set_rect (nth k cur d0) (nth k ws w0).
Proof. intros L. unfold relayout. change w0 with ((fun p => set_rect (snd p) (fst p)) (w0, d0)) at 1.
  rewrite map_nth, combine_nth by congruence. reflexivity.
Qed.

Lemma relayout_ids ws cur : length cur = length ws -> ids (relayout ws cur) = ids ws.
Proof. revert cur; induction ws as [|w ws IH]; intros [|r cur] L; simpl in *; try discriminate; auto.
  f_equal. apply IH. lia.
Qed.

Lemma rect_eqb_eq a b : rect_eqb a b = true -> a = b.
Proof. destruct a as [x1 y1 w1 h1], b as [x2 y2 w2 h2]. unfold rect_eqb, span_eq. simpl.
  intros H. apply andb_true_iff in H as [[H1 H2]%andb_true_iff [H3 H4]%andb_true_iff].
  apply Z.eqb_eq in H1, H2, H3, H4. subst. reflexivity.
Qed.

(* leaf_nb lists exactly the windows touching a in the layout cur. *)
Lemma leaf_nb_spec ws cur a b : spans_ok ws -> length cur = length ws -> Forall (fun r => 0 <= rw r /\ 0 <= rh r) cur ->
  (a < length ws)%nat -> In b (leaf_nb ws cur a) <-> (b < length ws)%nat /\ touch (nth a cur d0) (nth b cur d0) = true.
Proof. intros Sp Lc Fc Ha. unfold leaf_nb. cbv zeta.
  assert (Fb : Forall (fun r => 0 <= rw r /\ 0 <= rh r) (map wrect ws)).
  { apply Forall_forall. intros r (w & <- & Hw)%in_map_iff. apply Sp, Hw. }
  assert (Na : 0 <= rw (nth a cur d0) /\ 0 <= rh (nth a cur d0)).
  { rewrite Forall_forall in Fc. apply Fc, nth_In. lia. }
  rewrite (touching_scan _ _ Fb (proj1 Na) (proj2 Na)), in_app_iff, !filter_In, !in_seq, length_map, Lc. cbv beta.
  split.
  - intros [[[Hb T] R] | [[Hb R] T]].
    + apply rect_eqb_eq in R. rewrite <- R in T. split; [lia | exact T].
    + split; [lia | exact T].
  - intros [Hb T]. destruct (rect_eqb (nth b cur d0) (nth b (map wrect ws) d0)) eqn:R.
    + left. split; [split; [lia | rewrite <- (rect_eqb_eq _ _ R); exact T] | reflexivity].
    + right. split; [split; [lia | reflexivity] | exact T].
Qed.

Lemma leaf_pass_gap ws cur (B : nat -> option nat) : NoDup (ids ws) -> length cur = length ws ->
  (forall a, (a < length ws)%nat -> forall b, In b (leaf_nb ws cur a) <->
     (b < length ws)%nat /\ touch (nth a cur d0) (nth b cur d0) = true) ->
  leaf_pass (number (map (fun w => B (wid w)) ws) []) (flood (length cur) (leaf_nb ws cur)) [] O = true <->
  split_gap B ws (relayout ws cur).
Proof. intros Nd Lc Hnb. rewrite Lc. set (part := flood (length ws) (leaf_nb ws cur)).
  assert (Lr : length (relayout ws cur) = length ws).
  { pose proof (f_equal (@length nat) (relayout_ids ws cur Lc)) as H. unfold ids in H. rewrite !length_map in H. exact H. }
  assert (Nd' : NoDup (ids (relayout ws cur))) by (rewrite relayout_ids; auto).
  assert (FP : forall x y, (x < length ws)%nat -> (y < length ws)%nat ->
    (get part x = get part y <-> connected (relayout ws cur) (wid (nth x ws w0)) (wid (nth y ws w0)) = true)).
  { intros x y Hx Hy. assert (Ew : forall z, wid (nth z ws w0) = wid (nth z (relayout ws cur) w0))
      by (intros z; rewrite relayout_nth by exact Lc; reflexivity).
    rewrite !Ew, (connected_reach (relayout ws cur) (fun a b => touch (nth a cur d0) (nth b cur d0))); try rewrite Lr; auto.
    - exact (proj2 (flood_parts (length ws) (leaf_nb ws cur) (fun a b => touch (nth a cur d0) (nth b cur d0))
        (fun a b => touch_sym _ _) Hnb x y Hx Hy)).
    - intros a b. rewrite !relayout_nth by exact Lc. reflexivity. }
  destruct (number_map (map (fun w => B (wid w)) ws) []) as (F & _ & HinF & En). rewrite En.
  set (g := fun l => match posk F l with Some i => i | None => O end).
  assert (Ginj : forall l1 l2, In l1 F -> In l2 F -> g l1 = g l2 -> l1 = l2).
  { intros l1 l2 H1 H2. destruct (posk_in F l1 H1) as [i1 P1]. destruct (posk_in F l2 H2) as [i2 P2].
    unfold g. rewrite P1, P2. intros <-. rewrite <- (posk_nth _ _ _ P1). exact (posk_nth _ _ _ P2). }
  assert (Isl : forall k, (k < length ws)%nat ->
    nth k (map (option_map g) (map (fun w => B (wid w)) ws)) None = option_map g (B (wid (nth k ws w0)))).
  { intros k Hk. rewrite (nth_map_lt _ _ None) by (rewrite length_map; exact Hk).
    rewrite (nth_map_lt _ _ w0) by exact Hk. reflexivity. }
  assert (InF : forall k l, (k < length ws)%nat -> B (wid (nth k ws w0)) = Some l -> In l F).
  { intros k l Hk E. apply HinF. rewrite <- E. apply (in_map (fun w => B (wid w))), nth_In, Hk. }
  rewrite leaf_pass_gpass, gpass_conflict; [| apply opt_eqb_iff | intros ? ? []].
  unfold conflict, split_gap. rewrite relayout_ids by exact Lc. split.
  - intros (e1 & e2 & [[]|H1] & [[]|H2] & Ef & Ns).
    apply leaf_entries_in in H1 as (k1 & Hk1 & I1 & S1). apply leaf_entries_in in H2 as (k2 & Hk2 & I2 & S2).
    rewrite !length_map in Hk1, Hk2. rewrite Isl in I1, I2 by assumption. rewrite Nat.add_0_l in S1, S2.
    destruct (B (wid (nth k1 ws w0))) as [l1|] eqn:B1; [|discriminate I1].
    destruct (B (wid (nth k2 ws w0))) as [l2|] eqn:B2; [|discriminate I2]. injection I1 as I1. injection I2 as I2.
    assert (l1 = l2) as <- by (apply Ginj; [apply (InF k1) | apply (InF k2) | congruence]; assumption).
    exists (wid (nth k1 ws w0)), (wid (nth k2 ws w0)).
    split; [apply nth_in_ids, Hk1|]. split; [apply nth_in_ids, Hk2|]. split; [rewrite B1; discriminate|].
    split; [congruence|]. split; [apply nth_in_ids, Hk1|]. split; [apply nth_in_ids, Hk2|].
    destruct (connected _ _ _) eqn:C; [|reflexivity]. exfalso. apply Ns. rewrite S1, S2. apply FP; assumption.
  - intros (a & b & Ha & Hb & Na & E & _ & _ & C).
    apply in_ids_nth in Ha as (k1 & Hk1 & <-). apply in_ids_nth in Hb as (k2 & Hk2 & <-).
    destruct (B (wid (nth k1 ws w0))) as [l|] eqn:B1; [|congruence]. exists (g l, get part k1), (g l, get part k2).
    split; [right; apply leaf_entries_in; exists k1; rewrite !length_map, Isl, B1 by exact Hk1;
            split; [exact Hk1 | split; reflexivity]|].
    split; [right; apply leaf_entries_in; exists k2; rewrite !length_map, Isl, <- E by exact Hk2;
            split; [exact Hk2 | split; reflexivity]|].
    split; [reflexivity|]. cbn [snd]. intros G. apply FP in G; [congruence | assumption | assumption].
Qed.

Theorem leaf_splits_spec ws movers cur :
  NoDup (ids ws) -> spans_ok ws -> length cur = length ws ->
  Forall (fun r => 0 <= rw r /\ 0 <= rh r) cur ->
  leaf_splits ws movers cur = splits ws (relayout ws cur) movers.
Proof. intros Nd Sp Lc Fc. destruct (islands_spec ws movers Nd Sp) as [HB HBc].
  assert (Nd' : NoDup (ids (relayout ws cur))) by (rewrite relayout_ids; auto).
  apply eq_true_iff_eq. unfold leaf_splits, island.
  rewrite (leaf_pass_gap ws cur (lookup (islands_fast ws movers)) Nd Lc
    (fun a Ha b => leaf_nb_spec ws cur a b Sp Lc Fc Ha)).
  symmetry. exact (splits_gap ws (relayout ws cur) movers _ Nd' HB HBc).
Qed.

(* ---------- place::Sets, Search::mend and Search::mends ---------- *)

Fixpoint set_nth {A} (l : list A) (i : nat) (v : A) : list A :=
  match l, i with
  | [], _ => []
  | _ :: l', O => v :: l'
  | x :: l', S i' => x :: set_nth l' i' v
  end.

(* Sets: each position's parent. find halves the path on its way up (self.0[x] = self.0[self.0[x]],
   then x = self.0[x]); union hangs the greater root under the lesser. No parent lies above its
   position, so the Rust loop ends within length p rounds, the fuel here. *)
Fixpoint sfind (fuel : nat) (p : list nat) (x : nat) : list nat * nat :=
  match fuel with
  | O => (p, x)
  | S f => if Nat.eqb (nth x p x) x then (p, x)
           else let g := nth (nth x p x) p (nth x p x) in sfind f (set_nth p x g) g
  end.
Definition sunion (p : list nat) (e : nat * nat) :=
  let '(p1, a) := sfind (length p) p (fst e) in
  let '(p2, b) := sfind (length p1) p1 (snd e) in
  if Nat.eqb a b then p2 else set_nth p2 (Nat.max a b) (Nat.min a b).
(* root: (0..n).map(|x| sets.find(x)), each find halving paths for the next. *)
Fixpoint sroots (p : list nat) (xs : list nat) : list nat :=
  match xs with [] => [] | x :: xs' => let '(p', r) := sfind (length p) p x in r :: sroots p' xs' end.

(* Search::mend at a step placing k: a window stayed when it lies at its rectangle before (k does),
   moved otherwise. The unions: each stayed window but k with its later stayed sides before but k;
   each moved window with the stayed windows but k touching it (range tree over the layout before)
   and with the later moved ones touching it. Rust walks the moved windows in the order they moved,
   ascending here: the same pairs are joined, and only the classes of the roots matter. *)
Definition stayed ws cur b := rect_eqb (nth b cur d0) (nth b (map wrect ws) d0).
Definition moved_of ws cur := filter (fun b => negb (stayed ws cur b)) (seq 0 (length cur)).
Definition mend_edges (ws : list window) (cur : list rect) (k : nat) : list (nat * nat) :=
  let ix := index_new (map wrect ws) in
  flat_map (fun a => map (fun b => (a, b)) (filter (fun b => (a <? b)%nat && negb (Nat.eqb b k) && stayed ws cur b)
      (touching ix (nth a (map wrect ws) d0))))
    (filter (fun a => negb (Nat.eqb a k) && stayed ws cur a) (seq 0 (length cur))) ++
  flat_map (fun b => map (fun a => (a, b)) (filter (fun a => negb (Nat.eqb a k) && stayed ws cur a) (touching ix (nth b cur d0))) ++
      map (fun b2 => (b, b2)) (filter (fun b2 => (b <? b2)%nat && touch (nth b cur d0) (nth b2 cur d0)) (moved_of ws cur)))
    (moved_of ws cur).

(* lie[i]: the distinct roots of island i's windows but k, in order; must: the lists of two or more
   and k's own island's when not empty. Rust keeps one list per numbered island (count); the
   positions bound the numbers, and an island with no window has an empty list, never kept. *)
Definition distinct_by (keep : nat -> bool) (f : nat -> nat) (xs : list nat) :=
  fold_left (fun l x => if keep x && negb (mem (f x) l) then l ++ [f x] else l) xs [].
Definition lie_of (isl : list (option nat)) (root : list nat) (k i : nat) :=
  distinct_by (fun x => negb (Nat.eqb x k) && opt_eqb (nth x isl None) (Some i)) (fun x => nth x root O) (seq 0 (length isl)).
Definition must_of isl root k :=
  map (lie_of isl root k) (filter (fun i => (2 <=? length (lie_of isl root k i))%nat ||
    opt_eqb (nth k isl None) (Some i) && negb (Nat.eqb (length (lie_of isl root k i)) 0)) (seq 0 (length isl))).
Definition mend ws movers (cur : list rect) (k : nat) : list nat * list (list nat) :=
  let root := sroots (fold_left sunion (mend_edges ws cur k) (seq 0 (length cur))) (seq 0 (length cur)) in
  (root, must_of (island ws movers) root k).

(* Search::mends: the roots of the windows but k touching c (stayed ones through the range tree,
   moved ones by a scan), and whether some list in must holds a root among none of them. *)
Definition mend_touched ws cur k (root : list nat) c :=
  map (fun a => nth a root O) (filter (fun a => negb (Nat.eqb a k) && stayed ws cur a) (touching (index_new (map wrect ws)) c)) ++
  map (fun b => nth b root O) (filter (fun b => touch c (nth b cur d0)) (moved_of ws cur)).
Definition mends ws cur k (m : list nat * list (list nat)) c :=
  let '(root, must) := m in existsb (fun l => existsb (fun r => negb (mem r (mend_touched ws cur k root c))) l) must.

(* ---------- Sets: after the unions, two positions share a root exactly when the pairs join them ---------- *)

Lemma set_nth_length {A} (l : list A) i v : length (set_nth l i v) = length l.
Proof. revert i; induction l as [|x l IH]; intros [|i]; simpl; rewrite ?IH; reflexivity. Qed.

Lemma nth_set_nth {A} (l : list A) i v y d : (i < length l)%nat -> nth y (set_nth l i v) d = if Nat.eqb y i then v else nth y l d.
Proof. revert i y; induction l as [|x l IH]; intros [|i] [|y] H; simpl in *; try (exfalso; lia); auto. apply IH. lia. Qed.

Lemma strong_ind (P : nat -> Prop) : (forall x, (forall y, (y < x)%nat -> P y) -> P x) -> forall x, P x.
Proof.
  intros H x. assert (G : forall m y, (y < m)%nat -> P y).
  { induction m as [|m IH]; intros y Hy; [exfalso; lia|]. apply H. intros z Hz. apply IH. lia. }
  apply (G (S x)). lia.
Qed.

(* rt p x: the root above x, with enough fuel when parents never lie above their positions. *)
Fixpoint climb (fuel : nat) (p : list nat) (x : nat) : nat :=
  match fuel with O => x | S f => if Nat.eqb (nth x p x) x then x else climb f p (nth x p x) end.
Definition rt p x := climb (S x) p x.
Definition down (p : list nat) := forall y, (nth y p y <= y)%nat.

Lemma rt_eq p x : down p -> rt p x = if Nat.eqb (nth x p x) x then x else rt p (nth x p x).
Proof.
  intros D. assert (C : forall f g z, (z < f)%nat -> (z < g)%nat -> climb f p z = climb g p z).
  { induction f as [|f IH]; intros [|g] z Hf Hg; try (exfalso; lia). simpl.
    destruct (Nat.eqb (nth z p z) z) eqn:E; [reflexivity|]. apply Nat.eqb_neq in E. pose proof (D z). apply IH; lia. }
  unfold rt at 1. cbn [climb]. destruct (Nat.eqb (nth x p x) x) eqn:E; [reflexivity|].
  apply Nat.eqb_neq in E. pose proof (D x). unfold rt. apply C; lia.
Qed.

Lemma rt_parent p y : down p -> rt p (nth y p y) = rt p y.
Proof.
  intros D. destruct (Nat.eqb (nth y p y) y) eqn:E; [apply Nat.eqb_eq in E; rewrite E; reflexivity|].
  rewrite (rt_eq p y D), E. reflexivity.
Qed.

Lemma rt_root p x : down p -> nth (rt p x) p (rt p x) = rt p x /\ (rt p x <= x)%nat.
Proof.
  intros D. induction x as [x IH] using strong_ind. rewrite (rt_eq p x D).
  destruct (Nat.eqb (nth x p x) x) eqn:E; [apply Nat.eqb_eq in E; split; [exact E | lia]|].
  apply Nat.eqb_neq in E. pose proof (D x). destruct (IH (nth x p x)) as [H1 H2]; [lia|]. split; [exact H1 | lia].
Qed.

Lemma sfind_spec f : forall p x, down p -> (x < f)%nat ->
  down (fst (sfind f p x)) /\ length (fst (sfind f p x)) = length p /\ snd (sfind f p x) = rt p x /\
  (forall y, rt (fst (sfind f p x)) y = rt p y).
Proof.
  induction f as [|f IH]; intros p x D Hx; [exfalso; lia|]. simpl.
  destruct (Nat.eqb (nth x p x) x) eqn:E.
  - simpl. split; [exact D | split; [reflexivity | split; [rewrite (rt_eq p x D), E; reflexivity | intros; reflexivity]]].
  - apply Nat.eqb_neq in E. pose proof (D x) as Dx. pose proof (D (nth x p x)) as Dg.
    assert (Lx : (x < length p)%nat).
    { destruct (Nat.lt_ge_cases x (length p)) as [L|L]; [exact L|]. rewrite nth_overflow in E by exact L. congruence. }
    set (g := nth (nth x p x) p (nth x p x)) in *.
    assert (D' : down (set_nth p x g)).
    { intros y. rewrite nth_set_nth by exact Lx. destruct (Nat.eqb y x) eqn:Y; [apply Nat.eqb_eq in Y; lia | apply D]. }
    assert (Rg : rt p g = rt p x) by (unfold g; rewrite !rt_parent by exact D; reflexivity).
    assert (R' : forall y, rt (set_nth p x g) y = rt p y).
    { intros y. induction y as [y IHy] using strong_ind. rewrite (rt_eq _ y D'), nth_set_nth by exact Lx.
      destruct (Nat.eqb y x) eqn:Y; cbv beta iota.
      - apply Nat.eqb_eq in Y; subst y. replace (Nat.eqb g x) with false by (symmetry; apply Nat.eqb_neq; lia).
        rewrite IHy by lia. exact Rg.
      - destruct (Nat.eqb (nth y p y) y) eqn:E'.
        + rewrite (rt_eq p y D), E'. reflexivity.
        + apply Nat.eqb_neq in E'. pose proof (D y). rewrite IHy by lia. apply rt_parent, D. }
    destruct (IH (set_nth p x g) g D' ltac:(lia)) as (D2 & L2 & R2 & E2).
    split; [exact D2 | split; [rewrite L2, set_nth_length; reflexivity |
      split; [rewrite R2, R'; exact Rg | intros y; rewrite E2, R'; reflexivity]]].
Qed.

Lemma rt_hang p r s : down p -> (r < length p)%nat -> nth r p r = r -> nth s p s = s -> (s < r)%nat ->
  down (set_nth p r s) /\ forall y, rt (set_nth p r s) y = if Nat.eqb (rt p y) r then s else rt p y.
Proof.
  intros D Lr Hr Hs Lt.
  assert (D' : down (set_nth p r s)).
  { intros y. rewrite nth_set_nth by exact Lr. destruct (Nat.eqb y r) eqn:Y; [apply Nat.eqb_eq in Y; lia | apply D]. }
  assert (Rr : rt p r = r) by (rewrite (rt_eq p r D), Hr, Nat.eqb_refl; reflexivity).
  assert (Rs : rt p s = s) by (rewrite (rt_eq p s D), Hs, Nat.eqb_refl; reflexivity).
  split; [exact D'|]. intros y. induction y as [y IH] using strong_ind.
  rewrite (rt_eq _ y D'), nth_set_nth by exact Lr. destruct (Nat.eqb y r) eqn:Y; cbv beta iota.
  - apply Nat.eqb_eq in Y. subst y. replace (Nat.eqb s r) with false by (symmetry; apply Nat.eqb_neq; lia).
    rewrite IH, Rr, Rs, Nat.eqb_refl by lia. destruct (Nat.eqb s r); reflexivity.
  - destruct (Nat.eqb (nth y p y) y) eqn:E.
    + assert (rt p y = y) as -> by (rewrite (rt_eq p y D), E; reflexivity). rewrite Y. reflexivity.
    + apply Nat.eqb_neq in E. pose proof (D y). rewrite IH by lia. rewrite rt_parent by exact D. reflexivity.
Qed.

Lemma sunion_spec p a b : down p -> (a < length p)%nat -> (b < length p)%nat ->
  down (sunion p (a, b)) /\ length (sunion p (a, b)) = length p /\
  forall x y, rt (sunion p (a, b)) x = rt (sunion p (a, b)) y <->
    rt p x = rt p y \/ ((rt p x = rt p a \/ rt p x = rt p b) /\ (rt p y = rt p a \/ rt p y = rt p b)).
Proof.
  intros D Ha Hb. unfold sunion. cbn [fst snd].
  destruct (sfind_spec (length p) p a D Ha) as (D1 & L1 & R1 & E1).
  destruct (sfind (length p) p a) as [p1 ra] eqn:F1. cbn [fst snd] in D1, L1, R1, E1. cbv beta iota.
  destruct (sfind_spec (length p1) p1 b D1 ltac:(lia)) as (D2 & L2 & R2 & E2).
  destruct (sfind (length p1) p1 b) as [p2 rb] eqn:F2. cbn [fst snd] in D2, L2, R2, E2. cbv beta iota.
  rewrite E1 in R2. subst ra rb.
  assert (E : forall y, rt p2 y = rt p y) by (intros y; rewrite E2, E1; reflexivity).
  destruct (Nat.eqb (rt p a) (rt p b)) eqn:Q.
  - apply Nat.eqb_eq in Q. split; [exact D2 | split; [lia|]]. intros x y. rewrite !E.
    split; [intros H; left; exact H | intros [H|[[H1|H1] [H2|H2]]]; congruence].
  - apply Nat.eqb_neq in Q. pose proof (rt_root p2 a D2) as [Ra La]. pose proof (rt_root p2 b D2) as [Rb Lb].
    rewrite !E in Ra, La, Rb, Lb.
    assert (Hr : nth (Nat.max (rt p a) (rt p b)) p2 (Nat.max (rt p a) (rt p b)) = Nat.max (rt p a) (rt p b))
      by (destruct (Nat.max_spec (rt p a) (rt p b)) as [[_ ->]|[_ ->]]; assumption).
    assert (Hs : nth (Nat.min (rt p a) (rt p b)) p2 (Nat.min (rt p a) (rt p b)) = Nat.min (rt p a) (rt p b))
      by (destruct (Nat.min_spec (rt p a) (rt p b)) as [[_ ->]|[_ ->]]; assumption).
    destruct (rt_hang p2 (Nat.max (rt p a) (rt p b)) (Nat.min (rt p a) (rt p b)) D2 ltac:(lia) Hr Hs ltac:(lia)) as [D3 H3].
    split; [exact D3 | split; [rewrite set_nth_length; lia|]]. intros x y. rewrite !H3, !E.
    destruct (Nat.eqb_spec (rt p x) (Nat.max (rt p a) (rt p b))), (Nat.eqb_spec (rt p y) (Nat.max (rt p a) (rt p b)));
      split; intros; lia.
Qed.

Lemma sroots_spec p xs : down p -> (forall x, In x xs -> (x < length p)%nat) -> sroots p xs = map (rt p) xs.
Proof.
  revert p; induction xs as [|x xs IH]; intros p D H; [reflexivity|]. cbn [sroots map].
  destruct (sfind_spec (length p) p x D (H x (or_introl eq_refl))) as (D' & L' & R' & E').
  destruct (sfind (length p) p x) as [p' r] eqn:F. cbn [fst snd] in D', L', R', E'. cbv beta iota. subst r. f_equal.
  rewrite IH; [apply map_ext; exact E' | exact D' | intros z Hz; rewrite L'; apply H; right; exact Hz].
Qed.

Lemma reach_ext n A1 A2 a b : (forall x y, A1 x y = A2 x y) -> reach n A1 a b -> reach n A2 a b.
Proof.
  intros H R. induction R as [|b c R IH Hc E]; [constructor|].
  apply (reach_step _ _ _ b); [exact IH | exact Hc | rewrite <- H; exact E].
Qed.

Lemma reach_mono n A1 A2 a b : (a < n)%nat -> (forall x y, (x < n)%nat -> (y < n)%nat -> A1 x y = true -> reach n A2 x y) ->
  reach n A1 a b -> reach n A2 a b.
Proof.
  intros Ha H R. induction R as [|b c R IH Hc E]; [constructor|].
  apply (reach_trans _ _ _ b); [exact IH | apply H; auto; apply (reach_lt n A1 a b); auto].
Qed.

Lemma reach_into n A u v : reach n A u v -> (forall b, A b v = false) -> u = v.
Proof. intros R. destruct R as [|b c R Hc E]; intros H; [reflexivity | rewrite H in E; discriminate]. Qed.

(* Adding edges that join only windows of a set H, connected through the new edges and closed under
   the old ones, merges H into one class. *)
Lemma reach_merge n A A' (H : nat -> Prop) : (forall x y, A x y = A y x) ->
  (forall x y, A x y = true -> A' x y = true) ->
  (forall x y, (x < n)%nat -> (y < n)%nat -> A' x y = true -> A x y = false -> H x /\ H y) ->
  (forall x y, (x < n)%nat -> (y < n)%nat -> A x y = true -> H x -> H y) ->
  (forall x y, (x < n)%nat -> (y < n)%nat -> H x -> H y -> reach n A' x y) ->
  forall u v, (u < n)%nat -> (v < n)%nat -> (reach n A' u v <-> reach n A u v \/ (H u /\ H v)).
Proof.
  intros S Sub Ext Cl Con u v Hu Hv. split.
  - assert (Back : forall w, reach n A u w -> H w -> H u).
    { intros w R. induction R as [|b c R IH Hc E]; [auto|]. intros Hc'. apply IH.
      apply (Cl c b); auto; [apply (reach_lt n A u b); auto | rewrite S; exact E]. }
    intros R. clear Hv. induction R as [|b c R IH Hc E]; [left; constructor|].
    assert (Hb : (b < n)%nat) by (apply (reach_lt n A' u b); auto).
    destruct (A b c) eqn:Ab.
    + destruct IH as [IH|[H1 H2]]; [left; apply (reach_step _ _ _ b); auto | right; split; [exact H1 | apply (Cl b c); auto]].
    + destruct (Ext b c Hb Hc E Ab) as [H1 H2]. right. split; [|exact H2].
      destruct IH as [IH|[IH _]]; [exact (Back b IH H1) | exact IH].
  - intros [R|[H1 H2]]; [|apply Con; auto].
    apply (reach_mono n A A' u v Hu); auto. intros x y _ Hy E.
    apply (reach_step _ _ _ x); [constructor | exact Hy | apply Sub, E].
Qed.

Definition edge (e : nat * nat) x y := Nat.eqb (fst e) x && Nat.eqb (snd e) y || Nat.eqb (fst e) y && Nat.eqb (snd e) x.
Definition adj_of (E : list (nat * nat)) x y := existsb (fun e => edge e x y) E.

Lemma adj_of_iff E x y : adj_of E x y = true <-> In (x, y) E \/ In (y, x) E.
Proof.
  unfold adj_of, edge. rewrite existsb_exists. split.
  - intros ([u v] & H & B). simpl in B. apply orb_true_iff in B as [B|B]; apply andb_true_iff in B as [B1 B2];
      apply Nat.eqb_eq in B1, B2; subst; auto.
  - intros [H|H]; [exists (x, y) | exists (y, x)]; simpl; split; auto; rewrite !Nat.eqb_refl; [reflexivity | apply orb_true_r].
Qed.

(* The unions of a list of pairs within 0..n leave x and y on one root exactly when the pairs
   join them. *)
Lemma unions_spec n : forall E p A, down p -> length p = n -> (forall x y, A x y = A y x) ->
  (forall e, In e E -> (fst e < n)%nat /\ (snd e < n)%nat) ->
  (forall x y, (x < n)%nat -> (y < n)%nat -> rt p x = rt p y <-> reach n A x y) ->
  down (fold_left sunion E p) /\ length (fold_left sunion E p) = n /\
  forall x y, (x < n)%nat -> (y < n)%nat ->
    rt (fold_left sunion E p) x = rt (fold_left sunion E p) y <-> reach n (fun x y => A x y || adj_of E x y) x y.
Proof.
  induction E as [|[a b] E IH]; intros p A D L S HE R; cbn [fold_left].
  - split; [exact D | split; [exact L |]]. intros x y Hx Hy. rewrite R by assumption.
    split; apply reach_ext; intros u v; cbv beta; destruct (A u v); reflexivity.
  - destruct (HE (a, b) (or_introl eq_refl)) as [Ha Hb]. cbn [fst snd] in Ha, Hb.
    destruct (sunion_spec p a b D ltac:(lia) ltac:(lia)) as (D1 & L1 & R1).
    set (A1 := fun x y => A x y || edge (a, b) x y).
    assert (S1 : forall x y, A1 x y = A1 y x).
    { intros x y. unfold A1, edge. rewrite S. cbn [fst snd].
      destruct (Nat.eqb a x), (Nat.eqb b y), (Nat.eqb a y), (Nat.eqb b x); reflexivity. }
    assert (St : forall x y, (x < n)%nat -> (y < n)%nat -> A x y = true -> reach n A1 x y).
    { intros x y _ Hy E0. apply (reach_step _ _ _ x); [constructor | exact Hy | unfold A1; rewrite E0; reflexivity]. }
    assert (Ta : forall w, (w < n)%nat -> reach n A w a \/ reach n A w b -> reach n A1 w a).
    { intros w Hw [H|H]; [exact (reach_mono n A A1 w a Hw St H)|].
      apply (reach_step _ _ _ b); [exact (reach_mono n A A1 w b Hw St H) | exact Ha |].
      unfold A1, edge. cbn [fst snd]. rewrite !Nat.eqb_refl. destruct (A b a), (Nat.eqb a b), (Nat.eqb b a); reflexivity. }
    destruct (IH (sunion p (a, b)) A1 D1 ltac:(lia) S1) as (D2 & L2 & R2).
    + intros e He. apply HE. right. exact He.
    + intros x y Hx Hy. rewrite R1, !R by assumption. symmetry.
      apply (reach_merge n A A1 (fun u => reach n A u a \/ reach n A u b)); [exact S | | | | | exact Hx | exact Hy].
      * intros u v E0. unfold A1. rewrite E0. reflexivity.
      * intros u v Hu Hv E0 E1. unfold A1, edge in E0. rewrite E1 in E0. cbn [fst snd orb] in E0.
        apply orb_true_iff in E0 as [E0|E0]; apply andb_true_iff in E0 as [E2 E3]; apply Nat.eqb_eq in E2, E3; subst;
          split; (left + right); apply reach_refl.
      * intros u v Hu Hv E0 [H|H]; [left|right]. all: apply (reach_trans _ _ _ u); [|exact H].
        all: apply (reach_step _ _ _ v); [constructor | exact Hu | rewrite S; exact E0].
      * intros u v Hu Hv Hu' Hv'. apply (reach_trans _ _ _ a); [apply Ta; auto|].
        apply (reach_sym n A1 v a S1 Hv). apply Ta; auto.
    + split; [exact D2 | split; [exact L2 |]]. intros x y Hx Hy. rewrite R2 by assumption.
      split; apply reach_ext; intros u v; unfold A1; cbn [adj_of existsb]; rewrite orb_assoc; reflexivity.
Qed.

(* ---------- mend's edges are the touching pairs without k; mends answers splits ---------- *)

Lemma rect_eqb_refl r : rect_eqb r r = true.
Proof. unfold rect_eqb, span_eq. rewrite !Z.eqb_refl. reflexivity. Qed.

Lemma nth_nonneg rs a : Forall (fun r => 0 <= rw r /\ 0 <= rh r) rs -> 0 <= rw (nth a rs d0) /\ 0 <= rh (nth a rs d0).
Proof.
  intros F. destruct (Nat.lt_ge_cases a (length rs)) as [L|L];
    [rewrite Forall_forall in F; apply F, nth_In, L | rewrite nth_overflow by exact L; simpl; lia].
Qed.

Lemma befores_nonneg ws : spans_ok ws -> Forall (fun r => 0 <= rw r /\ 0 <= rh r) (map wrect ws).
Proof. intros Sp. apply Forall_forall. intros r (w & <- & Hw)%in_map_iff. apply Sp, Hw. Qed.

Lemma touching_iff rs r b : Forall (fun r => 0 <= rw r /\ 0 <= rh r) rs -> 0 <= rw r /\ 0 <= rh r ->
  In b (touching (index_new rs) r) <-> (b < length rs)%nat /\ touch r (nth b rs d0) = true.
Proof. intros F [H1 H2]. rewrite (touching_scan rs r F H1 H2), filter_In, in_seq. split; intros [A B]; split; auto; lia. Qed.

Lemma stayed_eq ws cur b : stayed ws cur b = true -> nth b cur d0 = nth b (map wrect ws) d0.
Proof. apply rect_eqb_eq. Qed.

Lemma in_moved ws cur b : length cur = length ws -> In b (moved_of ws cur) <-> (b < length ws)%nat /\ stayed ws cur b = false.
Proof.
  intros Lc. unfold moved_of. rewrite filter_In, in_seq. cbv beta. rewrite negb_true_iff.
  split; intros [H1 H2]; split; auto; lia.
Qed.

Lemma mend_edges_sound ws cur k u v : spans_ok ws -> length cur = length ws ->
  Forall (fun r => 0 <= rw r /\ 0 <= rh r) cur -> stayed ws cur k = true -> In (u, v) (mend_edges ws cur k) ->
  u <> k /\ v <> k /\ (u < length ws)%nat /\ (v < length ws)%nat /\ touch (nth u cur d0) (nth v cur d0) = true.
Proof.
  intros Sp Lc Fc Ks H. pose proof (befores_nonneg ws Sp) as Fb. unfold mend_edges in H. apply in_app_iff in H as [H|H].
  - apply in_flat_map in H as (a & Ha & H). apply in_map_iff in H as (b & E & Hb). injection E as <- <-.
    apply filter_In in Ha as [Ha Pa]. apply in_seq in Ha. apply filter_In in Hb as [Hb Pb].
    rewrite touching_iff in Hb by first [assumption | apply nth_nonneg; assumption]. rewrite length_map in Hb.
    destruct Hb as [Hb T]. cbv beta in Pa, Pb. rewrite !andb_true_iff, negb_true_iff, Nat.eqb_neq in Pa.
    rewrite !andb_true_iff, negb_true_iff, Nat.eqb_neq, Nat.ltb_lt in Pb.
    destruct Pa as [Ak Sa], Pb as [[_ Bk] Sb]. rewrite (stayed_eq _ _ _ Sa), (stayed_eq _ _ _ Sb). repeat split; auto; lia.
  - apply in_flat_map in H as (b & Hb & H). apply in_moved in Hb as [Hb Sb]; [|exact Lc].
    assert (Bk : b <> k) by (intros ->; congruence). apply in_app_iff in H as [H|H].
    + apply in_map_iff in H as (a & E & Ha). injection E as <- <-. apply filter_In in Ha as [Ha Pa].
      rewrite touching_iff in Ha by first [assumption | apply nth_nonneg; assumption]. rewrite length_map in Ha.
      destruct Ha as [Ha T]. cbv beta in Pa. rewrite andb_true_iff, negb_true_iff, Nat.eqb_neq in Pa. destruct Pa as [Ak Sa].
      rewrite (stayed_eq _ _ _ Sa), touch_sym. repeat split; auto.
    + apply in_map_iff in H as (b2 & E & H2). injection E as <- <-. apply filter_In in H2 as [H2 T].
      apply in_moved in H2 as [H2 S2]; [|exact Lc]. cbv beta in T. apply andb_true_iff in T as [_ T].
      repeat split; auto. intros ->. congruence.
Qed.

Lemma mend_edges_complete ws cur k u v : spans_ok ws -> length cur = length ws ->
  Forall (fun r => 0 <= rw r /\ 0 <= rh r) cur -> stayed ws cur k = true -> u <> v -> (u < length ws)%nat ->
  (v < length ws)%nat -> u <> k -> v <> k -> touch (nth u cur d0) (nth v cur d0) = true ->
  In (u, v) (mend_edges ws cur k) \/ In (v, u) (mend_edges ws cur k).
Proof.
  intros Sp Lc Fc Ks. pose proof (befores_nonneg ws Sp) as Fb.
  assert (SS : forall a b, (a < b)%nat -> (b < length ws)%nat -> a <> k -> b <> k -> stayed ws cur a = true ->
    stayed ws cur b = true -> touch (nth a cur d0) (nth b cur d0) = true -> In (a, b) (mend_edges ws cur k)).
  { intros a b L Hb Ak Bk Sa Sb T. unfold mend_edges. apply in_app_iff. left. apply in_flat_map. exists a. split.
    - apply filter_In. split; [apply in_seq; lia | rewrite Sa, (proj2 (Nat.eqb_neq a k) Ak); reflexivity].
    - apply in_map_iff. exists b. split; [reflexivity|]. apply filter_In. split.
      + apply touching_iff; [exact Fb | apply nth_nonneg, Fb |].
        rewrite length_map, <- (stayed_eq _ _ _ Sa), <- (stayed_eq _ _ _ Sb). split; [exact Hb | exact T].
      + rewrite (proj2 (Nat.ltb_lt a b) L), (proj2 (Nat.eqb_neq b k) Bk), Sb. reflexivity. }
  assert (SM : forall a b, (a < length ws)%nat -> (b < length ws)%nat -> a <> k -> stayed ws cur a = true ->
    stayed ws cur b = false -> touch (nth a cur d0) (nth b cur d0) = true -> In (a, b) (mend_edges ws cur k)).
  { intros a b Ha Hb Ak Sa Sb T. unfold mend_edges. apply in_app_iff. right. apply in_flat_map. exists b.
    split; [apply in_moved; [exact Lc | split; assumption]|]. apply in_app_iff. left. apply in_map_iff.
    exists a. split; [reflexivity|]. apply filter_In. split.
    + apply touching_iff; [exact Fb | apply nth_nonneg, Fc |].
      rewrite length_map, <- (stayed_eq _ _ _ Sa), touch_sym. split; [exact Ha | exact T].
    + rewrite Sa, (proj2 (Nat.eqb_neq a k) Ak). reflexivity. }
  assert (MM : forall a b, (a < b)%nat -> (b < length ws)%nat -> stayed ws cur a = false -> stayed ws cur b = false ->
    touch (nth a cur d0) (nth b cur d0) = true -> In (a, b) (mend_edges ws cur k)).
  { intros a b L Hb Sa Sb T. unfold mend_edges. apply in_app_iff. right. apply in_flat_map. exists a.
    split; [apply in_moved; [exact Lc | split; [lia | exact Sa]]|]. apply in_app_iff. right. apply in_map_iff.
    exists b. split; [reflexivity|]. apply filter_In. split; [apply in_moved; [exact Lc | split; assumption]|].
    rewrite (proj2 (Nat.ltb_lt a b) L), T. reflexivity. }
  intros Ne Hu Hv Uk Vk T. pose proof T as T'. rewrite touch_sym in T'.
  destruct (stayed ws cur u) eqn:Su, (stayed ws cur v) eqn:Sv; destruct (Nat.lt_total u v) as [L|[E|L]];
    try contradiction; auto.
Qed.

Lemma mend_touched_iff ws cur k root c r : spans_ok ws -> length cur = length ws -> stayed ws cur k = true ->
  0 <= rw c /\ 0 <= rh c -> In r (mend_touched ws cur k root c) <->
  exists t, (t < length ws)%nat /\ t <> k /\ touch c (nth t cur d0) = true /\ nth t root O = r.
Proof.
  intros Sp Lc Ks Hc. pose proof (befores_nonneg ws Sp) as Fb. unfold mend_touched. rewrite in_app_iff, !in_map_iff. split.
  - intros [(a & <- & Ha)|(b & <- & Hb)].
    + apply filter_In in Ha as [Ha P]. cbv beta in P. rewrite touching_iff in Ha by assumption. rewrite length_map in Ha.
      apply andb_true_iff in P as [P S]. apply negb_true_iff, Nat.eqb_neq in P. destruct Ha as [Ha T].
      exists a. rewrite (stayed_eq _ _ _ S). repeat split; auto.
    + apply filter_In in Hb as [Hb T]. apply in_moved in Hb as [Hb S]; [|exact Lc]. exists b. repeat split; auto.
      intros ->. congruence.
  - intros (t & Ht & Tk & T & <-). destruct (stayed ws cur t) eqn:S.
    + left. exists t. split; [reflexivity|]. apply filter_In. split.
      * apply touching_iff; auto. rewrite length_map, <- (stayed_eq _ _ _ S). auto.
      * rewrite S, (proj2 (Nat.eqb_neq t k) Tk). reflexivity.
    + right. exists t. split; [reflexivity|]. apply filter_In. split; [apply in_moved; auto | exact T].
Qed.

(* The layout without k: touching pairs among the windows but k. *)
Definition adj_at (cur : list rect) a b := touch (nth a cur d0) (nth b cur d0).
Definition adj_but k (cur : list rect) a b := negb (Nat.eqb a k) && negb (Nat.eqb b k) && touch (nth a cur d0) (nth b cur d0).
Lemma adj_but_sym k cur a b : adj_but k cur a b = adj_but k cur b a.
Proof. unfold adj_but. rewrite touch_sym. destruct (Nat.eqb a k), (Nat.eqb b k); reflexivity. Qed.
Lemma adj_but_k k cur b : adj_but k cur b k = false.
Proof. unfold adj_but. rewrite Nat.eqb_refl. destruct (Nat.eqb b k); reflexivity. Qed.

Lemma mend_root ws movers cur k x y : spans_ok ws -> length cur = length ws ->
  Forall (fun r => 0 <= rw r /\ 0 <= rh r) cur -> stayed ws cur k = true -> (x < length ws)%nat -> (y < length ws)%nat ->
  nth x (fst (mend ws movers cur k)) O = nth y (fst (mend ws movers cur k)) O <-> reach (length ws) (adj_but k cur) x y.
Proof.
  intros Sp Lc Fc Ks Hx Hy. unfold mend. cbn [fst]. rewrite Lc.
  assert (Ns : forall z, nth z (seq 0 (length ws)) z = z).
  { intros z. destruct (Nat.lt_ge_cases z (length ws)) as [L|L];
      [rewrite seq_nth by exact L; reflexivity | apply nth_overflow; rewrite length_seq; exact L]. }
  assert (D0 : down (seq 0 (length ws))) by (intros z; rewrite Ns; lia).
  assert (R0 : forall z, rt (seq 0 (length ws)) z = z) by (intros z; rewrite (rt_eq _ z D0), Ns, Nat.eqb_refl; reflexivity).
  destruct (unions_spec (length ws) (mend_edges ws cur k) (seq 0 (length ws)) (fun _ _ => false) D0 (length_seq _ _)
    (fun _ _ => eq_refl)) as (Df & Lf & Rf).
  - intros [u v] He. destruct (mend_edges_sound ws cur k u v Sp Lc Fc Ks He) as (_ & _ & Hu & Hv & _). split; assumption.
  - intros u v Hu Hv. rewrite !R0. split; [intros ->; constructor | intros R; exact (reach_into _ _ _ _ R (fun _ => eq_refl))].
  - rewrite sroots_spec by first [exact Df | intros z Hz; apply in_seq in Hz; lia].
    rewrite (nth_map_lt _ _ x _ x), (nth_map_lt _ _ y _ y), !Ns, Rf by (rewrite ?length_seq; assumption).
    split; apply reach_mono; auto.
    + intros u v Hu Hv H. change (adj_of (mend_edges ws cur k) u v = true) in H.
      apply adj_of_iff in H as [H|H]; apply (mend_edges_sound ws cur k _ _ Sp Lc Fc Ks) in H as (Uk & Vk & _ & _ & T);
        apply (reach_step _ _ _ u); [constructor | exact Hv | unfold adj_but; rewrite (proj2 (Nat.eqb_neq _ _) Uk),
        (proj2 (Nat.eqb_neq _ _) Vk) | constructor | exact Hv | unfold adj_but; rewrite (proj2 (Nat.eqb_neq _ _) Uk),
        (proj2 (Nat.eqb_neq _ _) Vk); rewrite touch_sym]; exact T.
    + intros u v Hu Hv H. destruct (Nat.eq_dec u v) as [<-|Ne]; [constructor|].
      unfold adj_but in H. rewrite !andb_true_iff, !negb_true_iff, !Nat.eqb_neq in H. destruct H as [[Uk Vk] T].
      apply (reach_step _ _ _ u); [constructor | exact Hv |]. change (adj_of (mend_edges ws cur k) u v = true).
      apply adj_of_iff, (mend_edges_complete ws cur k); auto.
Qed.

(* Placing k on c: the layout's islands are those without k, with k merging every island it touches. *)
Definition hits n (cur : list rect) k c w :=
  w = k \/ exists t, (t < n)%nat /\ t <> k /\ touch c (nth t cur d0) = true /\ reach n (adj_but k cur) w t.

Lemma merge_k cur k c n : length cur = n -> (k < n)%nat -> forall u v, (u < n)%nat -> (v < n)%nat ->
  reach n (adj_at (set_nth cur k c)) u v <-> reach n (adj_but k cur) u v \/ (hits n cur k c u /\ hits n cur k c v).
Proof.
  intros Lc Hk.
  assert (Nt : forall y, nth y (set_nth cur k c) d0 = if Nat.eqb y k then c else nth y cur d0)
    by (intros; apply nth_set_nth; lia).
  assert (Sym' : forall a b, adj_at (set_nth cur k c) a b = adj_at (set_nth cur k c) b a) by (intros; apply touch_sym).
  assert (Sub : forall a b, adj_but k cur a b = true -> adj_at (set_nth cur k c) a b = true).
  { unfold adj_but, adj_at. intros a b H. rewrite !Nt. destruct (Nat.eqb a k), (Nat.eqb b k); try discriminate H; exact H. }
  assert (ToK : forall w, (w < n)%nat -> hits n cur k c w -> reach n (adj_at (set_nth cur k c)) w k).
  { intros w Hw [->|(t & Ht & Tk & T & R)]; [constructor|].
    apply (reach_step _ _ _ t); [|exact Hk|].
    - apply (reach_mono n (adj_but k cur)); auto. intros x y _ Hy E.
      apply (reach_step _ _ _ x); [constructor | exact Hy | apply Sub, E].
    - unfold adj_at. rewrite !Nt, Nat.eqb_refl, (proj2 (Nat.eqb_neq t k) Tk), touch_sym. exact T. }
  intros u v Hu Hv. apply (reach_merge n (adj_but k cur) (adj_at (set_nth cur k c)) (hits n cur k c));
    [apply adj_but_sym | exact Sub | | | | exact Hu | exact Hv].
  - intros a b Ha Hb E E0. unfold adj_at in E. unfold adj_but in E0. rewrite !Nt in E.
    destruct (Nat.eqb_spec a k) as [->|Ak], (Nat.eqb_spec b k) as [->|Bk]; cbn [negb andb] in E, E0.
    + split; left; reflexivity.
    + split; [left; reflexivity | right; exists b; split; [exact Hb | split; [exact Bk | split; [exact E | constructor]]]].
    + split; [right; exists a; split; [exact Ha | split; [exact Ak | split; [rewrite touch_sym; exact E | constructor]]] |
        left; reflexivity].
    + congruence.
  - intros a b Ha Hb E [->|(t & Ht & Tk & T & R)]; [rewrite (adj_but_sym k cur k b), adj_but_k in E; discriminate E|].
    right. exists t. split; [exact Ht | split; [exact Tk | split; [exact T |]]]. apply (reach_trans _ _ _ a); [|exact R].
    apply (reach_step _ _ _ b); [constructor | exact Ha | rewrite adj_but_sym; exact E].
  - intros a b Ha Hb H1 H2. apply (reach_trans _ _ _ k); [apply ToK; auto|].
    apply (reach_sym n _ b k); [exact Sym' | exact Hb | apply ToK; auto].
Qed.

(* ---------- islands before by position, lie and must ---------- *)

Lemma posk_lt L l i : posk L l = Some i -> (i < length L)%nat.
Proof.
  revert i; induction L as [|a L IH]; simpl; intros i; [discriminate|]. destruct (Nat.eqb a l); [intros H; injection H as <-; lia|].
  destruct (posk L l) as [j|] eqn:P; simpl; [|discriminate]. intros H; injection H as <-. specialize (IH j eq_refl). lia.
Qed.

Lemma number_bound labels : forall seen x i, nth x (number labels seen) None = Some i -> (i <= length seen + x)%nat.
Proof.
  induction labels as [|[l|] r IH]; intros seen x i; simpl.
  - destruct x; discriminate.
  - rewrite find_posk. destruct (posk seen l) as [j|] eqn:P; simpl; destruct x as [|x]; simpl.
    + intros H; injection H as <-. pose proof (posk_lt _ _ _ P). lia.
    + intros H. specialize (IH seen x i H). lia.
    + intros H; injection H as <-. lia.
    + intros H. specialize (IH (seen ++ [l]) x i H). rewrite length_app in IH. simpl in IH. lia.
  - destruct x as [|x]; simpl; [discriminate|]. intros H. specialize (IH seen x i H). lia.
Qed.

Lemma island_length ws movers : length (island ws movers) = length ws.
Proof.
  unfold island. destruct (number_map (map (fun w => lookup (islands_fast ws movers) (wid w)) ws) []) as (F & _ & _ & ->).
  rewrite !length_map. reflexivity.
Qed.

Lemma island_lt ws movers x i : nth x (island ws movers) None = Some i -> (i < length ws)%nat.
Proof.
  intros H. pose proof (number_bound _ _ _ _ H) as Bd. simpl in Bd.
  destruct (Nat.lt_ge_cases x (length ws)) as [L|L]; [lia|].
  rewrite nth_overflow in H by (rewrite island_length; exact L). discriminate.
Qed.

Lemma island_same ws movers x y : (x < length ws)%nat -> (y < length ws)%nat ->
  (exists i, nth x (island ws movers) None = Some i /\ nth y (island ws movers) None = Some i) <->
  lookup (islands_fast ws movers) (wid (nth x ws w0)) <> None /\
  lookup (islands_fast ws movers) (wid (nth x ws w0)) = lookup (islands_fast ws movers) (wid (nth y ws w0)).
Proof.
  intros Hx Hy. unfold island.
  destruct (number_map (map (fun w => lookup (islands_fast ws movers) (wid w)) ws) []) as (F & _ & HinF & ->).
  rewrite !(nth_map_lt _ _ None) by (rewrite length_map; assumption). rewrite !(nth_map_lt _ _ w0) by assumption.
  assert (InF : forall z l, (z < length ws)%nat -> lookup (islands_fast ws movers) (wid (nth z ws w0)) = Some l -> In l F).
  { intros z l Hz E. apply HinF. rewrite <- E. apply (in_map (fun w => lookup (islands_fast ws movers) (wid w))), nth_In, Hz. }
  split.
  - intros (i & Ex & Ey).
    destruct (lookup (islands_fast ws movers) (wid (nth x ws w0))) as [lx|] eqn:Bx; [|discriminate Ex].
    destruct (lookup (islands_fast ws movers) (wid (nth y ws w0))) as [ly|] eqn:By; [|discriminate Ey].
    split; [discriminate|]. f_equal. simpl in Ex, Ey. injection Ex as Ex. injection Ey as Ey.
    destruct (posk_in F lx (InF x lx Hx Bx)) as [i1 P1]. destruct (posk_in F ly (InF y ly Hy By)) as [i2 P2].
    rewrite P1 in Ex. rewrite P2 in Ey. subst i1 i2. rewrite <- (posk_nth _ _ _ P1). exact (posk_nth _ _ _ P2).
  - intros [N E]. rewrite <- E. destruct (lookup (islands_fast ws movers) (wid (nth x ws w0))) as [l|]; [|congruence].
    exists (match posk F l with Some i => i | None => O end). split; reflexivity.
Qed.

Lemma relayout_reach ws cur x y : NoDup (ids ws) -> length cur = length ws -> (x < length ws)%nat -> (y < length ws)%nat ->
  connected (relayout ws cur) (wid (nth x ws w0)) (wid (nth y ws w0)) = true <-> reach (length ws) (adj_at cur) x y.
Proof.
  intros Nd Lc Hx Hy.
  assert (Lr : length (relayout ws cur) = length ws) by (unfold relayout; rewrite length_map, length_combine; lia).
  assert (Ew : forall z, wid (nth z ws w0) = wid (nth z (relayout ws cur) w0))
    by (intros z; rewrite relayout_nth by exact Lc; reflexivity).
  rewrite !Ew, (connected_reach (relayout ws cur) (adj_at cur) x y); rewrite ?Lr; try reflexivity; auto.
  - rewrite relayout_ids; auto.
  - intros a b. unfold adj_at. rewrite !relayout_nth by exact Lc. reflexivity.
Qed.

Lemma splits_pos ws movers cur : NoDup (ids ws) -> spans_ok ws -> length cur = length ws ->
  splits ws (relayout ws cur) movers = true <-> exists x y i, (x < length ws)%nat /\ (y < length ws)%nat /\
    nth x (island ws movers) None = Some i /\ nth y (island ws movers) None = Some i /\ ~ reach (length ws) (adj_at cur) x y.
Proof.
  intros Nd Sp Lc. destruct (islands_spec ws movers Nd Sp) as [HB HBc].
  rewrite (splits_gap ws (relayout ws cur) movers _ ltac:(rewrite relayout_ids; auto) HB HBc).
  unfold split_gap. rewrite relayout_ids by exact Lc. split.
  - intros (a & b & Ha & Hb & Na & E & _ & _ & C). apply in_ids_nth in Ha as (x & Hx & <-). apply in_ids_nth in Hb as (y & Hy & <-).
    destruct (proj2 (island_same ws movers x y Hx Hy) (conj Na E)) as (i & Ix & Iy).
    exists x, y, i. repeat split; auto. rewrite <- relayout_reach by auto. congruence.
  - intros (x & y & i & Hx & Hy & Ix & Iy & NR).
    destruct (proj1 (island_same ws movers x y Hx Hy) (ex_intro _ i (conj Ix Iy))) as [Na E].
    exists (wid (nth x ws w0)), (wid (nth y ws w0)). repeat split; try apply nth_in_ids; auto.
    destruct (connected _ _ _) eqn:C; [|reflexivity]. exfalso. apply NR. apply relayout_reach; auto.
Qed.

Lemma distinct_fold (keep : nat -> bool) (f : nat -> nat) : forall xs l0, NoDup l0 ->
  NoDup (fold_left (fun l x => if keep x && negb (mem (f x) l) then l ++ [f x] else l) xs l0) /\
  forall r, In r (fold_left (fun l x => if keep x && negb (mem (f x) l) then l ++ [f x] else l) xs l0) <->
    In r l0 \/ exists x, In x xs /\ keep x = true /\ f x = r.
Proof.
  induction xs as [|x xs IH]; intros l0 N; cbn [fold_left].
  - split; [exact N|]. intros r. split; [intros H; left; exact H | intros [H|(y & [] & _)]; exact H].
  - destruct (keep x && negb (mem (f x) l0)) eqn:K; cbv beta iota.
    + apply andb_true_iff in K as [K M]. apply negb_true_iff in M.
      assert (N' : NoDup (l0 ++ [f x])) by (apply nodup_snoc; [exact N | intros H; apply mem_iff in H; congruence]).
      destruct (IH _ N') as [N2 R2]. split; [exact N2|]. intros r. rewrite R2, in_app_iff. simpl. split.
      * intros [[H|[H|[]]]|(y & Hy & Ky & Ey)]; [left; exact H | right; exists x | right; exists y]; auto.
      * intros [H|(y & [<-|Hy] & Ky & Ey)]; [left; left; exact H | left; right; left; exact Ey | right; exists y; auto].
    + destruct (IH _ N) as [N2 R2]. split; [exact N2|]. intros r. rewrite R2. simpl. split.
      * intros [H|(y & Hy & Ky & Ey)]; [left; exact H | right; exists y; auto].
      * intros [H|(y & [<-|Hy] & Ky & Ey)]; [left; exact H | | right; exists y; auto].
        left. rewrite Ky in K. simpl in K. apply negb_false_iff, mem_iff in K. rewrite <- Ey. exact K.
Qed.

Lemma lie_iff isl root k i r : In r (lie_of isl root k i) <->
  exists x, (x < length isl)%nat /\ x <> k /\ nth x isl None = Some i /\ nth x root O = r.
Proof.
  unfold lie_of. destruct (distinct_fold (fun x => negb (Nat.eqb x k) && opt_eqb (nth x isl None) (Some i))
    (fun x => nth x root O) (seq 0 (length isl)) [] ltac:(constructor)) as [_ R]. split.
  - intros H. apply R in H as [[]|(x & Hx & K & E)]. cbv beta in K, E. apply andb_true_iff in K as [K1 K2].
    apply negb_true_iff, Nat.eqb_neq in K1. apply opt_eqb_iff in K2. apply in_seq in Hx. exists x. repeat split; auto; lia.
  - intros (x & Hx & Xk & Ix & E). apply R. right. exists x. split; [apply in_seq; lia|]. split; [|exact E].
    cbv beta. rewrite (proj2 (Nat.eqb_neq x k) Xk), (proj2 (opt_eqb_iff _ _) Ix). reflexivity.
Qed.

Lemma lie_nodup isl root k i : NoDup (lie_of isl root k i).
Proof. exact (proj1 (distinct_fold _ _ (seq 0 (length isl)) [] ltac:(constructor))). Qed.

Lemma two_members (l : list nat) r : NoDup l -> In r l -> (2 <= length l)%nat <-> exists r', In r' l /\ r' <> r.
Proof.
  intros N H. split.
  - destruct l as [|a [|b l]]; simpl; intros L; try (exfalso; lia).
    inversion N as [|? ? Na _]; subst. destruct (Nat.eq_dec a r) as [<-|Ar].
    + exists b. split; [right; left; reflexivity | intros <-; apply Na; left; reflexivity].
    + exists a. split; [left; reflexivity | exact Ar].
  - intros (r' & H' & Ne). destruct l as [|a [|b l]]; simpl in *; [contradiction | lia | lia].
Qed.

(* Search::mends at a step placing k (not placed, lying at its rectangle before) on c answers splits
   of the layout it completes, cur with c at k. *)
Theorem mends_spec ws movers cur k c :
  NoDup (ids ws) -> spans_ok ws -> length cur = length ws -> Forall (fun r => 0 <= rw r /\ 0 <= rh r) cur ->
  0 <= rw c -> 0 <= rh c -> (k < length ws)%nat -> nth k cur d0 = wrect (nth k ws w0) ->
  mends ws cur k (mend ws movers cur k) c = splits ws (relayout ws (set_nth cur k c)) movers.
Proof.
  intros Nd Sp Lc Fc Hc1 Hc2 Hk Kst.
  assert (Ks : stayed ws cur k = true) by (unfold stayed; rewrite nth_wrect, Kst; apply rect_eqb_refl).
  pose proof (fun x y => mend_root ws movers cur k x y Sp Lc Fc Ks) as Hroot.
  pose proof (fun r => mend_touched_iff ws cur k (fst (mend ws movers cur k)) c r Sp Lc Ks (conj Hc1 Hc2)) as Htouch.
  assert (Hmust : snd (mend ws movers cur k) = must_of (island ws movers) (fst (mend ws movers cur k)) k) by reflexivity.
  apply eq_true_iff_eq. rewrite (splits_pos ws movers (set_nth cur k c) Nd Sp ltac:(rewrite set_nth_length; exact Lc)).
  unfold mends. destruct (mend ws movers cur k) as [root must]. cbn [fst snd] in Hroot, Htouch, Hmust. subst must.
  cbv beta iota. set (touched := mend_touched ws cur k root c) in *. set (isl := island ws movers) in *.
  assert (Li : length isl = length ws) by apply island_length.
  assert (Hm : forall u v, (u < length ws)%nat -> (v < length ws)%nat -> reach (length ws) (adj_at (set_nth cur k c)) u v <->
    reach (length ws) (adj_but k cur) u v \/ (hits (length ws) cur k c u /\ hits (length ws) cur k c v))
    by (apply merge_k; assumption).
  assert (Hit : forall x, (x < length ws)%nat -> hits (length ws) cur k c x <-> Nat.eqb x k || mem (nth x root O) touched = true).
  { intros x Hx. rewrite orb_true_iff, Nat.eqb_eq, mem_iff, Htouch. unfold hits. split.
    - intros [->|(t & Ht & Tk & T & R)]; [left; reflexivity | right]. exists t.
      split; [exact Ht | split; [exact Tk | split; [exact T |]]].
      apply Hroot; [exact Ht | exact Hx | exact (reach_sym _ _ x t (adj_but_sym k cur) Hx R)].
    - intros [->|(t & Ht & Tk & T & E)]; [left; reflexivity | right]. exists t.
      split; [exact Ht | split; [exact Tk | split; [exact T |]]].
      exact (reach_sym _ _ t x (adj_but_sym k cur) Ht (proj1 (Hroot t x Ht Hx) E)). }
  assert (Lie : forall i r, In r (lie_of isl root k i) <->
    exists x, (x < length ws)%nat /\ x <> k /\ nth x isl None = Some i /\ nth x root O = r).
  { intros i r. rewrite lie_iff, Li. reflexivity. }
  rewrite existsb_exists. unfold must_of. split.
  - intros (l & Hl & Hr). apply in_map_iff in Hl as (i & <- & Hi). apply filter_In in Hi as [_ Ci].
    apply existsb_exists in Hr as (r & Hr & Nt). apply negb_true_iff in Nt.
    pose proof Hr as Hr'. apply Lie in Hr' as (x & Hx & Xk & Ix & <-).
    assert (NH : ~ hits (length ws) cur k c x).
    { intros H. apply Hit in H; [|exact Hx]. rewrite (proj2 (Nat.eqb_neq x k) Xk), Nt in H. discriminate. }
    apply orb_true_iff in Ci as [Ci|Ci].
    + apply Nat.leb_le in Ci. destruct (proj1 (two_members _ _ (lie_nodup isl root k i) Hr) Ci) as (r' & Hr2 & Ne).
      apply Lie in Hr2 as (y & Hy & Yk & Iy & <-). exists x, y, i. repeat split; auto.
      rewrite Hm by auto. intros [R|[H _]]; [apply Ne; symmetry; apply Hroot; auto | exact (NH H)].
    + apply andb_true_iff in Ci as [Ci _]. apply opt_eqb_iff in Ci. exists x, k, i. repeat split; auto.
      rewrite Hm by auto. intros [R|[H _]]; [apply Xk, (reach_into _ _ _ _ R), adj_but_k | exact (NH H)].
  - intros (x & y & i & Hx & Hy & Ix & Iy & NR). rewrite Hm in NR by auto.
    assert (W : exists x y, (x < length ws)%nat /\ (y < length ws)%nat /\ nth x isl None = Some i /\ nth y isl None = Some i /\
      ~ reach (length ws) (adj_but k cur) x y /\ (Nat.eqb x k || mem (nth x root O) touched) = false).
    { destruct (Nat.eqb x k || mem (nth x root O) touched) eqn:Hxb; [destruct (Nat.eqb y k || mem (nth y root O) touched) eqn:Hyb|].
      - exfalso. apply NR. right. split; apply Hit; auto.
      - exists y, x. repeat split; auto; intros R; apply NR; left; exact (reach_sym _ _ y x (adj_but_sym k cur) Hy R).
      - exists x, y. repeat split; auto; intros R; apply NR; left; exact R. }
    clear x y Hx Hy Ix Iy NR. destruct W as (x & y & Hx & Hy & Ix & Iy & NR & Hb).
    apply orb_false_iff in Hb as [Xk Nt]. apply Nat.eqb_neq in Xk.
    assert (Hr : In (nth x root O) (lie_of isl root k i)) by (apply Lie; exists x; auto).
    exists (lie_of isl root k i). split.
    + apply in_map_iff. exists i. split; [reflexivity|]. apply filter_In.
      split; [apply in_seq; pose proof (island_lt ws movers x i Ix); lia|]. cbv beta. apply orb_true_iff.
      destruct (Nat.eq_dec y k) as [->|Yk].
      * right. rewrite (proj2 (opt_eqb_iff _ _) Iy). destruct (lie_of isl root k i); [contradiction | reflexivity].
      * left. apply Nat.leb_le, (two_members _ _ (lie_nodup isl root k i) Hr). exists (nth y root O).
        split; [apply Lie; exists y; auto|]. intros E. apply NR, Hroot; auto.
    + apply existsb_exists. exists (nth x root O). split; [exact Hr | rewrite Nt; reflexivity].
Qed.

(* ---------- C51: a close or a shrink splits no island ---------- *)

(* Two windows connected in L stay connected in any L2 that holds every window of L. *)
Lemma connected_sub L L2 a b : (forall x, In x L -> In x L2) -> connected L a b = true -> connected L2 a b = true.
Proof.
  intros Sub H. unfold connected in *. apply mem_iff in H. apply mem_iff.
  assert (K : forall k seen, (forall i, In i seen -> In i (grow (length L2) L2 [a])) ->
    forall i, In i (grow k L seen) -> In i (grow (length L2) L2 [a])).
  { induction k as [|k IH]; intros seen Sd; [exact Sd|]. rewrite grow_S. apply IH. intros i Hi.
    apply in_app_or in Hi as [Hi|Hi]; [apply Sd, Hi|].
    apply in_map_iff in Hi as (w & <- & Hw). unfold gstep in Hw. apply filter_In in Hw as [Hw P].
    apply andb_true_iff in P as [_ P]. apply existsb_exists in P as (v & Hv & P). apply andb_true_iff in P as [M T].
    apply mem_iff, Sd in M. exact (grow_full L2 [a] v w (Sub v Hv) (Sub w Hw) M T). }
  apply (K (length L) [a]); [|exact H]. intros i [<-|[]]. apply grow_mono. left. reflexivity.
Qed.

(* A layout that keeps every window but the movers as it was splits nothing. *)
Lemma splits_sub ws ws' movers : (forall x, In x (but movers ws) -> In x ws') -> splits ws ws' movers = false.
Proof.
  intros Sub. unfold splits. apply not_true_is_false. intros H.
  apply existsb_exists in H as (a & _ & H). apply existsb_exists in H as (b & _ & H).
  apply andb_true_iff in H as [H N]. apply andb_true_iff in H as [H _]. apply andb_true_iff in H as [H _].
  apply andb_true_iff in H as [_ C]. rewrite (connected_sub _ _ _ _ Sub C) in N. discriminate.
Qed.

Lemma but_one_kept i ws x : In x (but [i] ws) -> In x ws /\ wid x <> i.
Proof.
  intros Hx. apply filter_In in Hx as [Hx N]. split; [exact Hx|]. intros E. rewrite E in N.
  cbn [mem existsb] in N. rewrite Nat.eqb_refl in N. discriminate.
Qed.

(* C51: a close (of a window that was no bridge) or a shrink never splits an island: two windows
   connected without it before are connected after, whether the gap stays open or fills. *)
Theorem fills_split_no_island e c i c' :
  (forall g freed, remove e c i g = Some c' -> rect_of i (windows c) = Some freed -> is_bridge (windows c) i = false ->
     splits (windows c) (windows c') [i] = false) /\
  (forall ed d p w, resize e c i ed d p = Some (true, c') -> d < 0 -> window_of i (windows c) = Some w ->
     splits (windows c) (windows c') [i] = false).
Proof.
  split.
  - intros g freed H Hr Hb. rewrite (proj1 (Claims.remove_shape _ _ _ _ _ _ Hr H)). unfold closing. cbv zeta. rewrite Hb.
    destruct (splits _ (closed _ _) _) eqn:S; [|exact S]. apply splits_sub. intros x Hx.
    destruct (but_one_kept _ _ _ Hx) as [Hx' N]. apply filter_In. split; [exact Hx'|].
    apply negb_true_iff, Nat.eqb_neq, N.
  - intros ed d p w H Dn Hw.
    destruct (Claims.resize_cases _ _ _ _ _ _ _ H) as (w' & Hw' & _ & _ & _ & [[Dp _] | [_ (freed & side & _ & ->)]]); [lia|].
    rewrite Hw in Hw'. injection Hw' as <-. destruct (splits (windows c) (fill freed side _) [i]) eqn:S; [|exact S]. apply splits_sub.
    intros x Hx. destruct (but_one_kept _ _ _ Hx) as [Hx' N]. unfold update. apply in_map_iff. exists x.
    split; [|exact Hx']. apply Nat.eqb_neq in N. rewrite N. reflexivity.
Qed.
