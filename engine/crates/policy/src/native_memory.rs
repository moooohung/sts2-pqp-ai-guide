use super::*;

fn bytes<T>(v:&Vec<T>)->usize {v.capacity()*std::mem::size_of::<T>()}
impl NativeDecision {
    fn owned_bytes(&self)->usize {
        let mut n=bytes(&self.rows)+bytes(&self.candidates)+bytes(&self.sources)+bytes(&self.targets)
            +bytes(&self.slots)+bytes(&self.actors)+bytes(&self.owners)+bytes(&self.relations)
            +bytes(&self.direct.seats)+bytes(&self.direct.ratios)+bytes(&self.groups)+bytes(&self.legacy)
            +bytes(&self.multiplicity)+bytes(&self.pool_multiplicity)+bytes(&self.message_multiplicity)
            +bytes(&self.degree);
        if let Some(old)=&self.projection {n+=std::mem::size_of::<Self>()+old.owned_bytes();}
        if let Some(stream)=&self.streamed {
            n+=std::mem::size_of::<streams::StreamedFacts>()+bytes(&stream.rows)+bytes(&stream.edges);
            for row in &stream.rows {n+=row.owned_bytes();}
            for edge in &stream.edges {n+=edge.input.owned_bytes()+bytes(&edge.main);}
        }
        n
    }
    pub fn allocation_stats(&self)->serde_json::Value {
        let (mut stream_rows,mut stream_relations,mut row_chunks,mut edge_chunks)=(0,0,0,0);
        if let Some(stream)=&self.streamed {
            row_chunks=stream.rows.len();edge_chunks=stream.edges.len();
            for row in &stream.rows {stream_rows+=row.rows.len();}
            for edge in &stream.edges {
                stream_rows+=edge.input.rows.len();stream_relations+=edge.input.relations.len();
            }
        }
        serde_json::json!({"rows":self.rows.len(),"row_capacity":self.rows.capacity(),
            "candidates":self.candidates.len(),"candidate_capacity":self.candidates.capacity(),
            "relations":self.relations.len(),"relation_capacity":self.relations.capacity(),
            "projection_rows":self.projection.as_ref().map_or(0,|p|p.rows.len()),
            "stream_rows":stream_rows,"stream_relations":stream_relations,
            "stream_row_chunks":row_chunks,"stream_edge_chunks":edge_chunks,
            "owned_buffer_bytes":self.owned_bytes(),
            "csr_bytes":0,"training_bucket_padding_bytes":0})
    }
}
