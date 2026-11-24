(* What push, fill and enforce_min guarantee, each for every direction. *)
From Stdlib Require Import ZArith List Bool Lia Permutation.
Import ListNotations.
From NewmCanvas Require Import Canvas.
Open Scope Z_scope.

Definition disjoint ws := forall a b, In a ws -> In b ws -> wid a <> wid b ->
  overlaps (wrect a) (wrect b) = false.
Definition spans_ok ws := forall w, In w ws -> 0 <= rw (wrect w) /\ 0 <= rh (wrect w).
Definition mins_ok e ws := forall w, In w ws ->
  fst (need e (wmin w)) <= rw (wrect w) /\ snd (need e (wmin w)) <= rh (wrect w).
Definition precedes d a b := match d with
  | Left | Right => (right a <=? ri b) && rows_overlap a b
  | Up | Down => (bottom a <=? rj b) && cols_overlap a b end.
Definition follows d a b := match d with Left | Right => right a <=? ri b | Up | Down => bottom a <=? rj b end.
Definition keeps_order d m ws ws' := forall a b a' b', In a ws -> In b ws -> In a' ws' -> In b' ws' ->
  wid a = wid a' -> wid b = wid b' -> wid a <> m -> wid b <> m ->
  precedes d (wrect a) (wrect b) = true -> follows d (wrect a') (wrect b') = true.

Example precedes_includes_touching :
  precedes Right (Rect 0 0 1 1) (Rect 1 0 1 1) = true /\ precedes Down (Rect 0 0 1 1) (Rect 0 1 1 1) = true /\
  follows Right (Rect 0 0 1 1) (Rect 1 0 1 1) = true /\ follows Down (Rect 0 0 1 1) (Rect 0 1 1 1) = true.
Proof. repeat split. Qed.

Ltac bools := repeat match goal with
  | H : (_ <? _) = true |- _ => apply Z.ltb_lt in H
  | H : (_ <? _) = false |- _ => apply Z.ltb_ge in H
  | H : (_ <=? _) = true |- _ => apply Z.leb_le in H
  | H : (_ <=? _) = false |- _ => apply Z.leb_gt in H
  | H : (_ =? _) = true |- _ => apply Z.eqb_eq in H
  | H : (_ =? _) = false |- _ => apply Z.eqb_neq in H
  | H : Nat.eqb _ _ = true |- _ => apply Nat.eqb_eq in H
  | H : Nat.eqb _ _ = false |- _ => apply Nat.eqb_neq in H
  | H : (_ && _) = true |- _ => apply andb_true_iff in H as [? ?]
  | H : (_ || _) = false |- _ => apply orb_false_iff in H as [? ?]
  | H : negb _ = true |- _ => apply negb_true_iff in H
  | H : negb _ = false |- _ => apply negb_false_iff in H
  end.

Lemma overlaps_sym a b : overlaps a b = overlaps b a.
Proof.
  unfold overlaps, rows_overlap, cols_overlap.
  destruct (rj a <? bottom b), (rj b <? bottom a), (ri a <? right b), (ri b <? right a); reflexivity.
Qed.

Lemma overlaps_iff a b : overlaps a b = true <->
  rj a < rj b + rh b /\ rj b < rj a + rh a /\ ri a < ri b + rw b /\ ri b < ri a + rw a.
Proof. unfold overlaps, rows_overlap, cols_overlap, right, bottom. rewrite !andb_true_iff, !Z.ltb_lt. tauto. Qed.

Lemma overlaps_shift a b di dj : overlaps (shifted a di dj) (shifted b di dj) = overlaps a b.
Proof. apply eq_true_iff_eq. rewrite !overlaps_iff. unfold shifted; simpl. split; intros; lia. Qed.

Definition within a b := ri b <= ri a /\ ri a + rw a <= ri b + rw b /\ rj b <= rj a /\ rj a + rh a <= rj b + rh b.
Lemma overlaps_within x a b : within a b -> overlaps x a = true -> overlaps x b = true.
Proof. unfold within; rewrite !overlaps_iff; lia. Qed.


(* Lists of windows. *)
Lemma nodup_unique ws a b : NoDup (ids ws) -> In a ws -> In b ws -> wid a = wid b -> a = b.
Proof.
  unfold ids; induction ws as [|x ws IH]; simpl; intros Hn Ha Hb He; [contradiction|].
  inversion Hn as [|? ? Hx Hn']; subst.
  destruct Ha as [<-|Ha], Hb as [<-|Hb]; auto.
  - exfalso; apply Hx; rewrite He; apply in_map; exact Hb.
  - exfalso; apply Hx; rewrite <- He; apply in_map; exact Ha.
Qed.

Lemma window_of_some i ws w : window_of i ws = Some w -> In w ws /\ wid w = i.
Proof. unfold window_of; intros H. apply find_some in H as [H1 H2]. apply Nat.eqb_eq in H2. auto. Qed.
Lemma window_of_none i ws : window_of i ws = None -> forall w, In w ws -> wid w <> i.
Proof.
  unfold window_of; intros H w Hw E. apply (find_none _ _ H) in Hw. rewrite E, Nat.eqb_refl in Hw. discriminate.
Qed.
Lemma rect_of_some i ws r : rect_of i ws = Some r -> exists w, In w ws /\ wid w = i /\ wrect w = r.
Proof.
  unfold rect_of; destruct (window_of i ws) eqn:E; simpl; intros H; [|discriminate].
  injection H as <-. apply window_of_some in E as [? ?]. eauto.
Qed.
Lemma window_of_in i ws w : NoDup (ids ws) -> In w ws -> wid w = i -> window_of i ws = Some w.
Proof.
  intros Hn Hw Hi. destruct (window_of i ws) as [w'|] eqn:E.
  - apply window_of_some in E as [E1 E2]. f_equal. apply (nodup_unique ws); congruence.
  - exfalso. exact (window_of_none _ _ E w Hw Hi).
Qed.
Lemma mem_iff i l : mem i l = true <-> In i l.
Proof. unfold mem. rewrite existsb_exists. split; [intros (x & H & E); apply Nat.eqb_eq in E; subst; auto|]. eauto using Nat.eqb_refl. Qed.

Lemma f2_map (R : window -> window -> Prop) f l : (forall x, In x l -> R x (f x)) -> Forall2 R l (map f l).
Proof. induction l; simpl; constructor; auto. Qed.
Lemma f2_diag (R : window -> window -> Prop) l : (forall x, In x l -> R x x) -> Forall2 R l l.
Proof.
  induction l as [|a l IH]; intros H; constructor; [apply H; left; reflexivity|].
  apply IH; intros x Hx; apply H; right; exact Hx.
Qed.
Lemma f2_trans (R S T : window -> window -> Prop) l1 l2 l3 :
  (forall x y z, R x y -> S y z -> T x z) -> Forall2 R l1 l2 -> Forall2 S l2 l3 -> Forall2 T l1 l3.
Proof. intros H A; revert l3; induction A; intros l3 B; inversion B; subst; constructor; eauto. Qed.
Lemma f2_in_r (R : window -> window -> Prop) l1 l2 y : Forall2 R l1 l2 -> In y l2 -> exists x, In x l1 /\ R x y.
Proof.
  intros A; induction A as [|x0 y0 k1 k2 R0 A IH]; simpl; intros Hin; [contradiction|].
  destruct Hin as [<-|Hin]; [eauto|]. destruct (IH Hin) as (x' & ? & ?); eauto.
Qed.
Lemma f2_ids (R : window -> window -> Prop) l1 l2 : (forall x y, R x y -> wid x = wid y) -> Forall2 R l1 l2 -> ids l1 = ids l2.
Proof. unfold ids; intros H A; induction A; simpl; f_equal; auto. Qed.
Lemma f2_window_of (R : window -> window -> Prop) l1 l2 i : (forall x y, R x y -> wid x = wid y) -> Forall2 R l1 l2 ->
  match window_of i l1, window_of i l2 with Some a, Some b => R a b | None, None => True | _, _ => False end.
Proof.
  intros H A; induction A as [|x y l1 l2 Rxy A IH]; simpl; [exact I|].
  unfold window_of in *; simpl. rewrite (H x y Rxy). destruct (Nat.eqb (wid y) i); auto.
Qed.

Definition same (a b : window) :=
  wid a = wid b /\ wmin a = wmin b /\ rw (wrect a) = rw (wrect b) /\ rh (wrect a) = rh (wrect b).
Lemma f2_same_refl l : Forall2 same l l.
Proof. induction l; constructor; unfold same; auto. Qed.
Lemma f2_same_trans l1 l2 l3 : Forall2 same l1 l2 -> Forall2 same l2 l3 -> Forall2 same l1 l3.
Proof. apply f2_trans. unfold same; intros; intuition congruence. Qed.

(* Facts that survive any change that keeps every window's id, minimum and span. *)
Lemma same_ids l1 l2 : Forall2 same l1 l2 -> ids l1 = ids l2.
Proof. apply f2_ids. intros x y H; apply H. Qed.
Lemma same_spans l1 l2 : Forall2 same l1 l2 -> spans_ok l1 -> spans_ok l2.
Proof.
  intros A H y Hy. destruct (f2_in_r _ _ _ _ A Hy) as (x & Hx & (_ & _ & E3 & E4)).
  rewrite <- E3, <- E4. apply H; exact Hx.
Qed.
Lemma same_mins e l1 l2 : Forall2 same l1 l2 -> mins_ok e l1 -> mins_ok e l2.
Proof.
  intros A H y Hy. destruct (f2_in_r _ _ _ _ A Hy) as (x & Hx & (_ & E2 & E3 & E4)).
  rewrite <- E2, <- E3, <- E4. apply H; exact Hx.
Qed.

(* push: a window only moves along d, each time to the far edge of what it passes, and a round
   that moves it leaves it at one of those edges, so the rounds stop. *)
Definition far d p := match d with Right => right p | Left => - ri p | Down => bottom p | Up => - rj p end.
Definition along d r r' := rw r' = rw r /\ rh r' = rh r /\
  match d with Left | Right => rj r' = rj r | Up | Down => ri r' = ri r end /\ lead d r <= lead d r'.
Definition clear d p r := shares d p r = false \/ far d p <= lead d r.

Lemma along_trans d a b c : along d a b -> along d b c -> along d a c.
Proof. unfold along; destruct d; lia. Qed.
Lemma along_past d p r : lead d r < far d p -> along d r (past d p r) /\ lead d (past d p r) = far d p.
Proof. unfold along, lead, far, past, right, bottom; destruct d; simpl; lia. Qed.
Lemma behind_far d r p : behind d r p = (lead d r <? far d p).
Proof. apply eq_true_iff_eq. unfold behind, lead, far, right, bottom; destruct d; rewrite !Z.ltb_lt; lia. Qed.
Lemma overlaps_far d r m : overlaps r m = true -> lead d r < far d m.
Proof. rewrite overlaps_iff. unfold lead, far, right, bottom; destruct d; lia. Qed.
Lemma clear_overlaps d p r : clear d p r -> overlaps r p = false.
Proof.
  intros H. destruct (overlaps r p) eqn:E; [|reflexivity]. apply overlaps_iff in E. exfalso.
  destruct H as [H|H]; [|unfold far, lead, right, bottom in H; destruct d; lia].
  destruct d; unfold shares, rows_overlap, cols_overlap, right, bottom in H;
    apply andb_false_iff in H as [H|H]; apply Z.ltb_ge in H; lia.
Qed.
Lemma shares_along d a a' b b' : along d a a' -> along d b b' -> shares d a' b' = shares d a b.
Proof.
  unfold along, shares, rows_overlap, cols_overlap, right, bottom.
  destruct d; intros (Wa & Ha & Ca & _) (Wb & Hb & Cb & _); congruence.
Qed.

Lemma pass_moves d l r : let r' := fold_left (fun r p => if shares d p r && behind d r p then past d p r else r) l r in
  along d r r' /\ (r' = r \/ lead d r < lead d r' /\ exists q, In q l /\ lead d r' = far d q) /\
  (r' = r -> forall p, In p l -> clear d p r).
Proof.
  revert r; induction l as [|p l IH]; intros r; simpl.
  - split; [unfold along; destruct d; lia|]. split; [auto|]. intros _ p [].
  - destruct (shares d p r && behind d r p) eqn:F.
    + apply andb_true_iff in F as [_ F]. rewrite behind_far, Z.ltb_lt in F.
      destruct (along_past d p r F) as [A1 L1]. destruct (IH (past d p r)) as (A & M & _).
      split; [exact (along_trans _ _ _ _ A1 A)|]. destruct A as (_ & _ & _ & L).
      split; [right; split; [lia|]|intros E; rewrite E in L; lia].
      destruct M as [->|(_ & q & Hq & Eq)]; [exists p|exists q]; simpl; auto.
    + destruct (IH r) as (A & M & C). split; [exact A|split].
      * destruct M as [E|(Lt & q & Hq & Eq)]; [left; exact E|right; split; [exact Lt|exists q; simpl; auto]].
      * intros E p' [<-|Hp]; [|exact (C E p' Hp)]. unfold clear. rewrite behind_far in F.
        apply andb_false_iff in F as [F|F]; [left; exact F|right; apply Z.ltb_ge in F; exact F].
Qed.

Lemma round_moves d m swept r : along d r (round d m swept r) /\
  (round d m swept r = r \/ lead d r < lead d (round d m swept r) /\
     exists q, In q (m :: swept) /\ lead d (round d m swept r) = far d q) /\
  (round d m swept r = r -> overlaps r m = false /\ forall p, In p swept -> clear d p r).
Proof.
  unfold round. destruct (overlaps r m) eqn:O.
  - pose proof (overlaps_far d r m O) as L. destruct (along_past d m r L) as [A1 L1].
    destruct (pass_moves d swept (past d m r)) as (A & M & _).
    split; [exact (along_trans _ _ _ _ A1 A)|]. destruct A as (_ & _ & _ & L2).
    split; [right; split; [lia|]|intros E; rewrite E in L2; lia].
    destruct M as [->|(_ & q & Hq & Eq)]; [exists m|exists q]; simpl; auto.
  - destruct (pass_moves d swept r) as (A & M & C). split; [exact A|split; [|auto]].
    destruct M as [E|(Lt & q & Hq & Eq)]; [left; exact E|right; split; [exact Lt|exists q; simpl; auto]].
Qed.

(* How many of the mover and the swept windows have their far edge past r's lead. *)
Definition ahead_count d m swept r := length (filter (fun q => lead d r <? far d q) (m :: swept)).

Lemma count_le (f g : rect -> bool) l : (forall y, In y l -> f y = true -> g y = true) ->
  (length (filter f l) <= length (filter g l))%nat.
Proof.
  induction l as [|y l IH]; simpl; intros H; [lia|]. specialize (IH (fun z Hz => H z (or_intror Hz))).
  destruct (f y) eqn:F; [rewrite (H y (or_introl eq_refl) F)|destruct (g y)]; simpl; lia.
Qed.
Lemma count_lt (f g : rect -> bool) l x : (forall y, In y l -> f y = true -> g y = true) ->
  In x l -> g x = true -> f x = false -> (length (filter f l) < length (filter g l))%nat.
Proof.
  induction l as [|y l IH]; simpl; intros H Hx Gx Fx; [contradiction|].
  pose proof (count_le f g l (fun z Hz => H z (or_intror Hz))) as Le.
  destruct Hx as [<-|Hx]; [rewrite Gx, Fx; simpl; lia|].
  specialize (IH (fun z Hz => H z (or_intror Hz)) Hx Gx Fx).
  destruct (f y) eqn:F; [rewrite (H y (or_introl eq_refl) F)|destruct (g y)]; simpl; lia.
Qed.
Lemma count_drops d m swept r q : lead d r < lead d (round d m swept r) -> In q (m :: swept) ->
  lead d (round d m swept r) = far d q -> (ahead_count d m swept (round d m swept r) < ahead_count d m swept r)%nat.
Proof.
  intros Lt Hq Eq. unfold ahead_count. apply (count_lt _ _ _ q); [|exact Hq|apply Z.ltb_lt; lia|apply Z.ltb_ge; lia].
  intros y _. rewrite !Z.ltb_lt. lia.
Qed.

Lemma settle_reaches d m swept n r : (ahead_count d m swept r <= n)%nat ->
  round d m swept (settle d m swept n r) = settle d m swept n r.
Proof.
  revert r; induction n as [|n IH]; intros r Hc; cbn [settle];
    destruct (round_moves d m swept r) as (_ & [E|(Lt & q & Hq & Eq)] & _).
  - exact E.
  - pose proof (count_drops d m swept r q Lt Hq Eq). lia.
  - assert (F : forall k, settle d m swept k r = r) by (induction k; cbn [settle]; rewrite ?E; auto).
    rewrite E, F. exact E.
  - apply IH. pose proof (count_drops d m swept r q Lt Hq Eq). lia.
Qed.

(* lib.rs repeats until the window stops moving: |swept| + 1 rounds always reach that point. *)
Theorem settle_stops d m swept r :
  round d m swept (settle d m swept (S (length swept)) r) = settle d m swept (S (length swept)) r.
Proof. apply settle_reaches. exact (filter_length_le _ (m :: swept)). Qed.

Lemma settle_along d m swept n r : along d r (settle d m swept n r).
Proof.
  revert r; induction n as [|n IH]; intros r; cbn [settle]; [unfold along; destruct d; lia|].
  exact (along_trans _ _ _ _ (proj1 (round_moves d m swept r)) (IH _)).
Qed.

(* The sweep: each window ends clear of the mover and of every window swept before it. *)
Lemma sweep_clear d mr swept order x : In x (sweep d mr swept order) ->
  overlaps (wrect x) mr = false /\ forall p, In p swept -> clear d p (wrect x).
Proof.
  revert swept; induction order as [|w o IH]; cbn [sweep]; intros swept Hx; [contradiction|].
  destruct Hx as [<-|Hx]; [exact (proj2 (proj2 (round_moves d mr swept _)) (settle_stops _ _ _ _))|].
  destruct (IH _ Hx) as [O C]. split; [exact O|]. intros p Hp; apply C, in_or_app; auto.
Qed.

Lemma sweep_shape d mr swept order : Forall2 (fun w x => wid w = wid x /\ wmin x = wmin w /\
  along d (wrect w) (wrect x)) order (sweep d mr swept order).
Proof.
  revert swept; induction order as [|w o IH]; intros swept; cbn [sweep]; constructor; [|apply IH].
  split; [reflexivity|split; [reflexivity|apply settle_along]].
Qed.

Lemma sweep_disjoint d mr swept order : disjoint (sweep d mr swept order).
Proof.
  revert swept; induction order as [|w o IH]; cbn [sweep]; intros swept a b Ha Hb Hab; [contradiction|].
  assert (Hr : In (settle d mr swept (S (length swept)) (wrect w)) (swept ++ [settle d mr swept (S (length swept)) (wrect w)]))
    by (apply in_or_app; simpl; auto).
  destruct Ha as [<-|Ha], Hb as [<-|Hb].
  - congruence.
  - rewrite overlaps_sym. exact (clear_overlaps d _ _ (proj2 (sweep_clear _ _ _ _ _ Hb) _ Hr)).
  - exact (clear_overlaps d _ _ (proj2 (sweep_clear _ _ _ _ _ Ha) _ Hr)).
  - exact (IH _ a b Ha Hb Hab).
Qed.

(* The order holds each window other than the mover once. *)
Lemma insert_perm d w l : Permutation (w :: l) (insert d w l).
Proof.
  induction l as [|x l IH]; simpl; [reflexivity|]. destruct (sooner d x w); [|reflexivity].
  exact (perm_trans (perm_swap x w l) (perm_skip x IH)).
Qed.
Lemma order_perm d m ws : Permutation (filter (fun w => negb (Nat.eqb (wid w) m)) ws) (order d m ws).
Proof.
  unfold order. induction (filter _ ws) as [|w l IH]; simpl; [reflexivity|].
  exact (perm_trans (perm_skip w IH) (insert_perm _ _ _)).
Qed.
Lemma order_in d m ws w : In w (order d m ws) <-> In w ws /\ wid w <> m.
Proof.
  rewrite <- (Nat.eqb_neq (wid w) m), <- negb_true_iff.
  transitivity (In w (filter (fun w => negb (Nat.eqb (wid w) m)) ws)); [|apply filter_In].
  split; apply Permutation_in; [apply Permutation_sym|]; apply order_perm.
Qed.
Lemma order_nodup d m ws : NoDup (ids ws) -> NoDup (ids (order d m ws)).
Proof.
  intros Hn. apply (Permutation_NoDup (Permutation_map wid (order_perm d m ws))). revert Hn; unfold ids.
  induction ws as [|w ws IH]; simpl; intros Hn; [constructor|]. inversion Hn as [|? ? Hw Hn']; subst.
  destruct (negb _); simpl; [constructor; [|auto]|auto].
  intros Hi; apply Hw. apply in_map_iff in Hi as (x & <- & Hx). apply filter_In in Hx as [Hx _]. apply in_map, Hx.
Qed.

(* Each window takes the rectangle the sweep gave it; the mover keeps its own. *)
Lemma sweep_ids d mr swept order : ids (sweep d mr swept order) = ids order.
Proof. symmetry. exact (f2_ids _ _ _ (fun x y H => proj1 H) (sweep_shape d mr swept order)). Qed.
Lemma put_wid done w : wid (put done w) = wid w.
Proof. unfold put. destruct (rect_of _ _); reflexivity. Qed.
Lemma put_mover d m mr ws w : wid w = m -> put (sweep d mr [] (order d m ws)) w = w.
Proof.
  intros E. unfold put, rect_of. destruct (window_of _ _) as [x|] eqn:Ex; [|reflexivity]. exfalso.
  apply window_of_some in Ex as [Hx Ex]. apply in_map with (f := wid) in Hx. rewrite sweep_ids, Ex, E in Hx.
  apply in_map_iff in Hx as (u & Eu & Hu). apply order_in in Hu. tauto.
Qed.
Lemma put_other d m mr ws w : NoDup (ids ws) -> In w ws -> wid w <> m -> exists x, In x (sweep d mr [] (order d m ws)) /\
  wid x = wid w /\ along d (wrect w) (wrect x) /\ put (sweep d mr [] (order d m ws)) w = set_rect (wrect x) w.
Proof.
  intros Hn Hw E. set (done := sweep d mr [] (order d m ws)).
  assert (Hi : In (wid w) (ids done)) by (unfold done; rewrite sweep_ids; apply in_map, order_in; auto).
  apply in_map_iff in Hi as (x0 & E0 & Hx0). unfold put, rect_of.
  destruct (window_of (wid w) done) as [x|] eqn:Ex; [|exfalso; exact (window_of_none _ _ Ex x0 Hx0 E0)].
  destruct (window_of_some _ _ _ Ex) as [Hx Ix]. destruct (f2_in_r _ _ _ _ (sweep_shape d mr [] _) Hx) as (u & Hu & Iu & _ & Au).
  assert (u = w) as -> by (apply (nodup_unique (order d m ws)); [apply order_nodup| | |]; auto; [apply order_in|]; auto; congruence).
  exists x. auto.
Qed.

(* A push leaves every window clear of every other, whatever overlapped before. *)
Theorem push_disjoint d m ws ws' : NoDup (ids ws) -> In m (ids ws) -> push d m ws = Some ws' -> disjoint ws'.
Proof.
  intros Hn Hm H. apply in_map_iff in Hm as (u & Eu & Hu).
  unfold push, rect_of in H. rewrite (window_of_in m ws u Hn Hu Eu) in H. simpl in H. injection H as <-.
  intros a' b' Ha Hb Hab. apply in_map_iff in Ha as (a & <- & Ha). apply in_map_iff in Hb as (b & <- & Hb).
  rewrite !put_wid in Hab.
  assert (Own : forall w, In w ws -> wid w = m -> wrect w = wrect u) by (intros w Hw E; f_equal; apply (nodup_unique ws); congruence).
  destruct (Nat.eq_dec (wid a) m) as [Am|Am], (Nat.eq_dec (wid b) m) as [Bm|Bm]; [congruence| | |].
  - destruct (put_other d m (wrect u) ws b Hn Hb Bm) as (x & Hx & _ & _ & ->).
    rewrite (put_mover d m (wrect u) ws a Am), (Own a Ha Am), overlaps_sym. exact (proj1 (sweep_clear _ _ _ _ _ Hx)).
  - destruct (put_other d m (wrect u) ws a Hn Ha Am) as (x & Hx & _ & _ & ->).
    rewrite (put_mover d m (wrect u) ws b Bm), (Own b Hb Bm). exact (proj1 (sweep_clear _ _ _ _ _ Hx)).
  - destruct (put_other d m (wrect u) ws a Hn Ha Am) as (x & Hx & Ex & _ & ->).
    destruct (put_other d m (wrect u) ws b Hn Hb Bm) as (y & Hy & Ey & _ & ->).
    apply (sweep_disjoint _ _ _ _ x y Hx Hy). congruence.
Qed.

Lemma push_same d m ws ws' : NoDup (ids ws) -> push d m ws = Some ws' -> Forall2 same ws ws'.
Proof.
  unfold push; intros Hn H. destruct (rect_of m ws) as [mr|]; injection H as <-; [|apply f2_same_refl].
  apply f2_map. intros w Hw. destruct (Nat.eq_dec (wid w) m) as [E|E].
  - rewrite (put_mover d m mr ws w E). unfold same; auto.
  - destruct (put_other d m mr ws w Hn Hw E) as (x & _ & _ & (W & H & _) & ->). unfold same; simpl; auto.
Qed.

(* Without unique ids push still keeps the mover where it is. *)
Definition same_at i (a b : window) := wid a = wid b /\ wmin a = wmin b /\
  (wid a = i -> rw (wrect a) = rw (wrect b) /\ rh (wrect a) = rh (wrect b)).
Lemma push_same_at d m ws ws' : push d m ws = Some ws' -> Forall2 (same_at m) ws ws'.
Proof.
  unfold push; intros H. destruct (rect_of m ws) as [mr|]; injection H as <-; [|apply f2_diag; unfold same_at; auto].
  apply f2_map. intros w _. destruct (Nat.eq_dec (wid w) m) as [E|E]; [rewrite (put_mover d m mr ws w E); unfold same_at; auto|].
  unfold same_at, put. destruct (rect_of _ _); simpl; repeat split; auto; intros; contradiction.
Qed.

(* fill: a chain slides together, and only into tiles no other window holds. *)
Lemma slide_same ch di dj n ws : Forall2 same ws (slide ch di dj n ws).
Proof.
  induction n; simpl; [apply f2_same_refl|]. destruct (slides _ _ _ _ _); auto.
  apply f2_map; intros w _. destruct (mem (wid w) ch); unfold same; simpl; auto.
Qed.
Lemma fill_same freed side ws : Forall2 same ws (fill freed side ws).
Proof.
  unfold fill. destruct (chain freed side ws); [apply f2_same_refl|].
  destruct (delta (opposite side)). apply slide_same.
Qed.

Lemma slid_disjoint ch di dj k ws : disjoint ws -> slides ch di dj k ws = true -> disjoint (slid ch di dj k ws).
Proof.
  intros Hd Hs a' b' Ha Hb Hab. unfold slid in Ha, Hb.
  apply in_map_iff in Ha as [a [<- Ha]]. apply in_map_iff in Hb as [b [<- Hb]].
  unfold slides in Hs. rewrite forallb_forall in Hs.
  destruct (mem (wid a) ch) eqn:Ma, (mem (wid b) ch) eqn:Mb; simpl in *.
  - rewrite overlaps_shift. apply Hd; auto.
  - specialize (Hs a Ha). rewrite Ma in Hs. simpl in Hs. unfold free in Hs. rewrite forallb_forall in Hs.
    specialize (Hs b Hb). rewrite Mb in Hs. simpl in Hs. rewrite overlaps_sym. apply negb_true_iff in Hs. exact Hs.
  - specialize (Hs b Hb). rewrite Mb in Hs. simpl in Hs. unfold free in Hs. rewrite forallb_forall in Hs.
    specialize (Hs a Ha). rewrite Ma in Hs. simpl in Hs. apply negb_true_iff in Hs. exact Hs.
  - apply Hd; auto.
Qed.

Theorem fill_disjoint freed side ws : disjoint ws -> disjoint (fill freed side ws).
Proof.
  intros Hd. unfold fill. destruct (chain freed side ws) as [|c ch]; [exact Hd|].
  destruct (delta (opposite side)) as [di dj]. generalize (Z.to_nat (if di =? 0 then rh freed else rw freed)).
  intros n; induction n; simpl; [exact Hd|]. destruct (slides _ _ _ _ _) eqn:E; auto. apply slid_disjoint; auto.
Qed.

(* Changing one window. *)
Lemma update_f2 (R : window -> window -> Prop) i f ws :
  (forall w, In w ws -> wid w = i -> R w (f w)) -> (forall w, In w ws -> wid w <> i -> R w w) -> Forall2 R ws (update i f ws).
Proof. intros H1 H2. apply f2_map. intros w Hw. destruct (Nat.eqb (wid w) i) eqn:E; bools; auto. Qed.


Lemma update_ids i f ws : (forall w, wid (f w) = wid w) -> ids (update i f ws) = ids ws.
Proof. intros Hf. unfold ids, update. rewrite map_map. apply map_ext. intros w. destruct (Nat.eqb (wid w) i); auto. Qed.

Lemma window_of_update i f ws w : (forall w, wid (f w) = wid w) -> window_of i ws = Some w -> window_of i (update i f ws) = Some (f w).
Proof.
  intros Hf. unfold window_of, update. induction ws as [|x ws IH]; simpl; [discriminate|].
  destruct (Nat.eqb (wid x) i) eqn:E; simpl; [rewrite Hf, E; congruence|]. rewrite E. exact IH.
Qed.

Lemma rect_of_update i f ws x : (forall w, wid (f w) = wid w) -> x <> i -> rect_of x (update i f ws) = rect_of x ws.
Proof.
  intros Hf Hx. unfold rect_of, window_of, update. induction ws as [|y ws IH]; simpl; [reflexivity|].
  destruct (Nat.eqb (wid y) i) eqn:E; simpl; rewrite ?Hf.
  - bools. destruct (Nat.eqb (wid y) x) eqn:E'; bools; [congruence|exact IH].
  - destruct (Nat.eqb (wid y) x); [reflexivity|exact IH].
Qed.

(* A window grows (or is placed) and pushes: everything ends disjoint, spans as set. *)
Definition regrown m r' (a b : window) := wid a = wid b /\ wmin a = wmin b /\
  (if Nat.eqb (wid a) m then rw (wrect b) = rw r' /\ rh (wrect b) = rh r'
   else rw (wrect b) = rw (wrect a) /\ rh (wrect b) = rh (wrect a)).

Lemma regrow d m r' ws ws' : NoDup (ids ws) -> spans_ok ws -> In m (ids ws) -> 0 <= rw r' -> 0 <= rh r' ->
  push d m (update m (set_rect r') ws) = Some ws' ->
  NoDup (ids ws') /\ spans_ok ws' /\ disjoint ws' /\ Forall2 (regrown m r') ws ws'.
Proof.
  intros Hn Hs Hi Hw Hh H.
  assert (Hu : Forall2 (regrown m r') ws (update m (set_rect r') ws)).
  { apply update_f2; intros w Hw' E; unfold regrown; simpl; repeat split.
    - rewrite E, Nat.eqb_refl. auto.
    - destruct (Nat.eqb (wid w) m) eqn:E'; bools; [congruence|auto]. }
  assert (Hn' : NoDup (ids (update m (set_rect r') ws))) by (rewrite update_ids; auto).
  assert (Hs' : spans_ok (update m (set_rect r') ws)).
  { intros w' Hw'. destruct (f2_in_r _ _ _ _ Hu Hw') as (w & Hw0 & (E1 & _ & E3)).
    destruct (Nat.eqb (wid w) m); destruct E3 as [-> ->]; [split; assumption | exact (Hs w Hw0)]. }
  pose proof (push_same _ _ _ _ Hn' H) as A.
  split; [|split; [|split]].
  - rewrite <- (same_ids _ _ A); exact Hn'.
  - exact (same_spans _ _ A Hs').
  - apply (push_disjoint d m _ _ Hn'); [rewrite update_ids; auto|exact H].
  - eapply f2_trans; [|exact Hu|exact A]. intros x y z (E1 & E2 & E3) (F1 & F2 & F3 & F4).
    unfold regrown. repeat split; try congruence. destruct (Nat.eqb (wid x) m); lia.
Qed.

Lemma nodup_snoc (l : list nat) x : NoDup l -> ~ In x l -> NoDup (l ++ [x]).
Proof.
  induction l as [|y l IH]; simpl; intros Hn Hx; [constructor; [intros []|constructor]|].
  inversion Hn as [|? ? Hy Hl]; subst. constructor.
  - rewrite in_app_iff; simpl. intros [H|[H|[]]]; [contradiction | apply Hx; left; congruence].
  - apply IH; auto.
Qed.

Lemma append_push d ws w ws' : NoDup (ids ws) -> ~ In (wid w) (ids ws) -> spans_ok ws ->
  0 <= rw (wrect w) -> 0 <= rh (wrect w) -> push d (wid w) (ws ++ [w]) = Some ws' ->
  NoDup (ids ws') /\ spans_ok ws' /\ disjoint ws' /\ Forall2 same (ws ++ [w]) ws'.
Proof.
  intros Hn Hi Hs Hw Hh H.
  assert (Hn' : NoDup (ids (ws ++ [w]))) by (unfold ids; rewrite map_app; apply nodup_snoc; auto).
  pose proof (push_same _ _ _ _ Hn' H) as A.
  assert (Hs' : spans_ok (ws ++ [w])) by (intros x Hx; apply in_app_or in Hx as [Hx|[<-|[]]]; auto).
  split; [rewrite <- (same_ids _ _ A); exact Hn'|split; [exact (same_spans _ _ A Hs')|split; [|exact A]]].
  apply (push_disjoint d (wid w) _ _ Hn'); [unfold ids; rewrite map_app; apply in_or_app; simpl; auto|exact H].
Qed.
