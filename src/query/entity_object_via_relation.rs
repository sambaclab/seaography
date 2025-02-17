use std::fmt::Debug;

use async_graphql::{
    dataloader::DataLoader,
    dynamic::{Field, FieldFuture, FieldValue, InputValue, ObjectAccessor, TypeRef, ValueAccessor},
    Error,
};
use heck::{ToLowerCamelCase, ToSnakeCase};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, DatabaseTransaction, EntityTrait,
    Iden, IntoActiveModel, Iterable, ModelTrait, PrimaryKeyToColumn, QueryFilter, Related,
    RelationDef,
};

#[cfg(not(feature = "offset-pagination"))]
use crate::ConnectionObjectBuilder;
use crate::{
    apply_memory_pagination, apply_order, apply_pagination, get_filter_conditions,
    prepare_active_model, BuilderContext, EntityInputBuilder, EntityObjectBuilder,
    FilterInputBuilder, GuardAction, HashableGroupKey, KeyComplex, NewOrderInputBuilder,
    OffsetInput, OneToManyLoader, OneToOneLoader, OrderInputBuilder, PageInput, PaginationInput,
    PaginationInputBuilder, ThanosRelationBuilder,
};

/// This builder produces a GraphQL field for an SeaORM entity related trait
/// that can be added to the entity object
pub struct EntityObjectViaRelationBuilder {
    pub context: &'static BuilderContext,
}

impl EntityObjectViaRelationBuilder {
    /// used to get a GraphQL field for an SeaORM entity related trait
    pub fn get_relation<T, R>(&self, name: &str) -> Field
    where
        T: Related<R>,
        T: EntityTrait,
        R: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <R as sea_orm::EntityTrait>::Model: Sync,
        <<T as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
        <<R as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
    {
        let name = if cfg!(feature = "field-snake-case") {
            name.to_snake_case()
        } else {
            name.to_lower_camel_case()
        };
        let context: &'static BuilderContext = self.context;
        let to_relation_definition = <T as Related<R>>::to();
        let (via_relation_definition, is_via_relation) = match <T as Related<R>>::via() {
            Some(def) => (def, true),
            None => (<T as Related<R>>::to(), false),
        };

        let entity_object_builder = EntityObjectBuilder { context };
        #[cfg(not(feature = "offset-pagination"))]
        let connection_object_builder = ConnectionObjectBuilder { context };
        let filter_input_builder = FilterInputBuilder { context };
        let order_input_builder = OrderInputBuilder { context };
        let new_order_input_builder = NewOrderInputBuilder { context };
        let object_name: String = entity_object_builder.type_name::<R>();
        #[cfg(feature = "offset-pagination")]
        let type_ref = TypeRef::named_list(&object_name);
        #[cfg(not(feature = "offset-pagination"))]
        let type_ref = TypeRef::named_nn(connection_object_builder.type_name(&object_name));

        #[cfg(feature = "offset-pagination")]
        let resolver_fn =
            |object: Vec<R::Model>| FieldValue::list(object.into_iter().map(FieldValue::owned_any));
        #[cfg(not(feature = "offset-pagination"))]
        let resolver_fn = |object: crate::Connection<R>| FieldValue::owned_any(object);
        let guard = self.context.guards.entity_guards.get(&object_name);

        let from_col = <T::Column as std::str::FromStr>::from_str(
            via_relation_definition
                .from_col
                .to_string()
                .to_snake_case()
                .as_str(),
        )
        .unwrap();

        let to_col = <R::Column as std::str::FromStr>::from_str(
            to_relation_definition
                .to_col
                .to_string()
                .to_snake_case()
                .as_str(),
        )
        .unwrap();

        let field = match via_relation_definition.is_owner {
            false => Field::new(name, TypeRef::named(&object_name), move |ctx| {
                FieldFuture::new(async move {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&ctx)
                    } else {
                        GuardAction::Allow
                    };

                    if let GuardAction::Block(reason) = guard_flag {
                        return match reason {
                            Some(reason) => {
                                Err::<Option<_>, async_graphql::Error>(Error::new(reason))
                            }
                            None => Err::<Option<_>, async_graphql::Error>(Error::new(
                                "Entity guard triggered.",
                            )),
                        };
                    }

                    let parent: &T::Model = ctx
                        .parent_value
                        .try_downcast_ref::<T::Model>()
                        .expect("Parent should exist");

                    let loader = ctx.data_unchecked::<DataLoader<OneToOneLoader<R>>>();

                    let stmt = if <T as Related<R>>::via().is_some() {
                        <T as Related<R>>::find_related()
                    } else {
                        R::find()
                    };

                    let filters = ctx.args.get(&context.entity_query_field.filters);
                    let filters = get_filter_conditions::<R>(context, filters);
                    let order_by = ctx.args.get(&context.entity_query_field.order_by);
                    let mut order_by = OrderInputBuilder { context }.parse_object::<R>(order_by);
                    let order = ctx.args.get(&context.entity_query_field.order);
                    let order = NewOrderInputBuilder { context }.parse_object::<R>(order);
                    order_by.extend(order);
                    let key = KeyComplex::<R> {
                        key: vec![parent.get(from_col)],
                        meta: HashableGroupKey::<R> {
                            stmt,
                            columns: vec![to_col],
                            filters: Some(filters),
                            order_by,
                        },
                    };

                    let data = loader.load_one(key).await?;

                    if let Some(data) = data {
                        Ok(Some(FieldValue::owned_any(data)))
                    } else {
                        Ok(None)
                    }
                })
            }),
            true => Field::new(name, type_ref, move |ctx| {
                let context: &'static BuilderContext = context;
                FieldFuture::new(async move {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&ctx)
                    } else {
                        GuardAction::Allow
                    };

                    if let GuardAction::Block(reason) = guard_flag {
                        return match reason {
                            Some(reason) => {
                                Err::<Option<_>, async_graphql::Error>(Error::new(reason))
                            }
                            None => Err::<Option<_>, async_graphql::Error>(Error::new(
                                "Entity guard triggered.",
                            )),
                        };
                    }

                    // FIXME: optimize union queries
                    // NOTE: each has unique query in order to apply pagination...
                    let parent: &T::Model = ctx
                        .parent_value
                        .try_downcast_ref::<T::Model>()
                        .expect("Parent should exist");

                    let stmt = if <T as Related<R>>::via().is_some() {
                        <T as Related<R>>::find_related()
                    } else {
                        R::find()
                    };

                    let filters = ctx.args.get(&context.entity_query_field.filters);
                    let filters = get_filter_conditions::<R>(context, filters);

                    let order_by = ctx.args.get(&context.entity_query_field.order_by);
                    let mut order_by = OrderInputBuilder { context }.parse_object::<R>(order_by);

                    let order = ctx.args.get(&context.entity_query_field.order);
                    let order = NewOrderInputBuilder { context }.parse_object::<R>(order);
                    order_by.extend(order);

                    let pagination = ctx.args.get(&context.entity_query_field.pagination);
                    let pagination = PaginationInputBuilder { context }.parse_object(pagination);
                    let first = ctx.args.get("first");
                    let pagination = match first {
                        Some(first_value) => match first_value.u64() {
                            Ok(first_num) => {
                                if let Some(offset) = pagination.offset {
                                    PaginationInput {
                                        offset: Some(OffsetInput {
                                            offset: offset.offset,
                                            limit: first_num,
                                        }),
                                        page: None,
                                        cursor: None,
                                    }
                                } else if let Some(page) = pagination.page {
                                    PaginationInput {
                                        offset: None,
                                        page: Some(PageInput {
                                            page: page.page,
                                            limit: first_num,
                                        }),
                                        cursor: None,
                                    }
                                } else {
                                    PaginationInput {
                                        offset: Some(OffsetInput {
                                            offset: 0,
                                            limit: first_num,
                                        }),
                                        page: None,
                                        cursor: None,
                                    }
                                }
                            }
                            _error => pagination,
                        },
                        None => pagination,
                    };
                    let db = ctx.data::<DatabaseConnection>()?;

                    let object = if is_via_relation {
                        // TODO optimize query
                        let condition = Condition::all().add(from_col.eq(parent.get(from_col)));

                        let stmt = stmt.filter(condition.add(filters));
                        let stmt = apply_order(stmt, order_by);
                        apply_pagination::<R>(db, stmt, pagination).await?
                    } else {
                        let loader = ctx.data_unchecked::<DataLoader<OneToManyLoader<R>>>();

                        let key = KeyComplex::<R> {
                            key: vec![parent.get(from_col)],
                            meta: HashableGroupKey::<R> {
                                stmt,
                                columns: vec![to_col],
                                filters: Some(filters),
                                order_by,
                            },
                        };

                        let values = loader.load_one(key).await?;
                        apply_memory_pagination::<R>(values, pagination)
                    };

                    Ok(Some(resolver_fn(object)))
                })
            }),
        };

        field
            .argument(InputValue::new(
                &context.entity_query_field.filters,
                TypeRef::named(filter_input_builder.type_name(&object_name)),
            ))
            .argument(InputValue::new(
                &context.entity_query_field.order_by,
                TypeRef::named(order_input_builder.type_name(&object_name)),
            ))
            .argument(InputValue::new(
                &self.context.entity_query_field.order,
                TypeRef::named(new_order_input_builder.type_name(&object_name)),
            ))
            .argument(InputValue::new(
                &context.entity_query_field.pagination,
                TypeRef::named(&context.pagination_input.type_name),
            ))
            .argument(InputValue::new("first", TypeRef::named(TypeRef::INT)))
    }

    pub fn get_relation_input<T, R>(&self, name: &str) -> (InputValue, InputValue)
    where
        T: Related<R>,
        T: EntityTrait,
        R: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <R as sea_orm::EntityTrait>::Model: Sync,
        <<T as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
        <<R as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
    {
        let name = if cfg!(feature = "field-snake-case") {
            name.to_snake_case()
        } else {
            name.to_lower_camel_case()
        };
        let context: &'static BuilderContext = self.context;

        let entity_input_builder = EntityInputBuilder { context };
        let via_relation_definition = match <T as Related<R>>::via() {
            Some(def) => def,
            None => <T as Related<R>>::to(),
        };

        let (object_insert_input_name, object_insert_update_name) = (
            entity_input_builder.insert_type_name::<R>(),
            entity_input_builder.update_type_name::<R>(),
        );
        match (
            via_relation_definition.is_owner,
            via_relation_definition.rel_type,
        ) {
            (true, sea_orm::RelationType::HasMany) => (
                InputValue::new(
                    name.clone(),
                    TypeRef::named_nn_list(object_insert_input_name),
                ),
                InputValue::new(name, TypeRef::named_nn_list(object_insert_update_name)),
            ),
            _ => (
                InputValue::new(name.clone(), TypeRef::named(object_insert_input_name)),
                InputValue::new(name, TypeRef::named(object_insert_update_name)),
            ),
        }
    }

    pub async fn insert_related<T, A, R, B, I>(
        &self,
        input_object: &ValueAccessor<'_>,
        owner: bool,
        upsert: bool,
        transaction: &DatabaseTransaction,
        related_entities: I,
    ) -> async_graphql::Result<()>
    where
        T: Related<R>,
        T: EntityTrait,
        R: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <R as sea_orm::EntityTrait>::Model: Sync,
        <<T as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
        <<R as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
        <T as EntityTrait>::Model: IntoActiveModel<A>,
        A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + std::marker::Send,
        <R as EntityTrait>::Model: IntoActiveModel<B>,
        B: ActiveModelTrait<Entity = R> + sea_orm::ActiveModelBehavior + std::marker::Send,
        I: IntoIterator + Clone,
        <I as IntoIterator>::Item: ThanosRelationBuilder + Debug,
    {
        let context = self.context;
        let entity_object_builder = EntityObjectBuilder { context };
        let entity_input_builder = EntityInputBuilder { context };

        let via_relation_definition = match <T as Related<R>>::via() {
            Some(def) => def,
            None => <T as Related<R>>::to(),
        };

        if owner != via_relation_definition.is_owner {
            let active_models = match (
                via_relation_definition.is_owner,
                via_relation_definition.rel_type.clone(),
            ) {
                (true, sea_orm::RelationType::HasMany) => {
                    let objs = input_object.list()?;
                    let mut active_models = vec![];
                    for val in objs.iter() {
                        let obj = val.object()?;
                        for related_entity in related_entities.clone() {
                            related_entity
                                .insert_related(context, &obj, transaction, true, upsert)
                                .await?;
                        }
                        let active_model = prepare_active_model::<R, B>(
                            &entity_input_builder,
                            &entity_object_builder,
                            &obj,
                        )?;
                        active_models.push(active_model);
                    }
                    active_models
                }
                _ => {
                    let obj = input_object.object()?;
                    for related_entity in related_entities.clone() {
                        related_entity
                            .insert_related(context, &obj, transaction, true, upsert)
                            .await?;
                    }
                    let active_model = prepare_active_model::<R, B>(
                        &entity_input_builder,
                        &entity_object_builder,
                        &obj,
                    )?;
                    vec![active_model]
                }
            };
            if upsert {
                R::insert_many(active_models).on_conflict(
                    sea_orm::sea_query::OnConflict::columns(
                        R::PrimaryKey::iter()
                            .map(|pk| pk.into_column())
                            .collect::<Vec<R::Column>>(),
                    )
                    .update_columns(R::Column::iter())
                    .to_owned(),
                )
            } else {
                R::insert_many(active_models)
            }
            .exec(transaction)
            .await?;

            match (
                via_relation_definition.is_owner,
                via_relation_definition.rel_type,
            ) {
                (true, sea_orm::RelationType::HasMany) => {
                    if let Ok(objs) = input_object.list() {
                        for val in objs.iter() {
                            if let Ok(obj) = val.object() {
                                for related_entity in related_entities.clone() {
                                    related_entity
                                        .insert_related(context, &obj, transaction, false, upsert)
                                        .await?;
                                }
                            }
                        }
                    } else {
                        return Err(async_graphql::Error::new("Invalid Input"));
                    }
                }
                _ => {
                    if let Ok(obj) = input_object.object() {
                        for related_entity in related_entities {
                            related_entity
                                .insert_related(context, &obj, transaction, false, upsert)
                                .await?;
                        }
                    } else {
                        return Err(async_graphql::Error::new("Invalid Input"));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn joiin<T, R>(
        &self,
        relation_definition: RelationDef,
        filter: Option<ValueAccessor>,
    ) -> RelationDef
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <<T as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
        R: EntityTrait,
        <R as sea_orm::EntityTrait>::Model: Sync,
        <<R as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
    {
        let filters = get_filter_conditions::<R>(self.context, filter);
        relation_definition.on_condition(move |_left, _right| filters.to_owned())
    }
}