//! .NET 9 List<T>.Sort ordering, including unstable equal-key permutations.
//! The pinned game's StableShuffle calls this before Fisher-Yates.
//! Adapted from dotnet/runtime v9.0.0 ArraySortHelper.cs (MIT, .NET Foundation).
use std::cmp::Ordering;

pub fn sort<T:Copy,F:Fn(T,T)->Ordering+Copy>(a:&mut[T],cmp:F) {
    if a.len()>1 {intro(a,2*(a.len().ilog2()+1),cmp);}
}
fn swap_greater<T:Copy,F:Fn(T,T)->Ordering>(a:&mut[T],i:usize,j:usize,cmp:F) {
    if cmp(a[i],a[j])==Ordering::Greater {a.swap(i,j);}
}
fn intro<T:Copy,F:Fn(T,T)->Ordering+Copy>(mut a:&mut[T],mut depth:u32,cmp:F) {
    while a.len()>1 {
        if a.len()<=16 {
            if a.len()==2 {swap_greater(a,0,1,cmp);return;}
            if a.len()==3 {
                swap_greater(a,0,1,cmp);swap_greater(a,0,2,cmp);swap_greater(a,1,2,cmp);return;
            }
            for i in 1..a.len() {
                let t=a[i];let mut j=i;
                while j>0 && cmp(t,a[j-1])==Ordering::Less {a[j]=a[j-1];j-=1;}
                a[j]=t;
            }
            return;
        }
        if depth==0 {heap(a,cmp);return;}
        depth-=1;
        let hi=a.len()-1;let mid=hi/2;
        swap_greater(a,0,mid,cmp);swap_greater(a,0,hi,cmp);swap_greater(a,mid,hi,cmp);
        let pivot=a[mid];a.swap(mid,hi-1);
        let mut left=0;let mut right=hi-1;
        loop {
            left+=1;while cmp(a[left],pivot)==Ordering::Less {left+=1;}
            right-=1;while cmp(pivot,a[right])==Ordering::Less {right-=1;}
            if left>=right {break;}
            a.swap(left,right);
        }
        if left!=hi-1 {a.swap(left,hi-1);}
        let (low,rest)=a.split_at_mut(left);intro(&mut rest[1..],depth,cmp);a=low;
    }
}
fn heap<T:Copy,F:Fn(T,T)->Ordering+Copy>(a:&mut[T],cmp:F) {
    let n=a.len();
    for i in (1..=n/2).rev() {down(a,i,n,cmp);}
    for i in (2..=n).rev() {a.swap(0,i-1);down(a,1,i-1,cmp);}
}
fn down<T:Copy,F:Fn(T,T)->Ordering+Copy>(a:&mut[T],mut i:usize,n:usize,cmp:F) {
    let d=a[i-1];
    while i<=n/2 {
        let mut child=2*i;
        if child<n && cmp(a[child-1],a[child])==Ordering::Less {child+=1;}
        if cmp(d,a[child-1])!=Ordering::Less {break;}
        a[i-1]=a[child-1];i=child;
    }
    a[i-1]=d;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_card_instances_follow_dotnet_three_element_sort_network() {
        // Uproar: Strike A, Strike B, Peck. .NET's 0/2 exchange reverses
        // the equal Strikes; a stable Rust sort incorrectly preserves A,B.
        let mut cards=[("STRIKE_DEFECT",0),("STRIKE_DEFECT",1),("PECK",2)];
        sort(&mut cards,|a,b|a.0.cmp(b.0));
        assert_eq!(cards,[("PECK",2),("STRIKE_DEFECT",1),("STRIKE_DEFECT",0)]);
    }
}
