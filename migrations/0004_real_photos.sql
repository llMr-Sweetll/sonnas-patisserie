-- Wire real product photography (from Sonna's own listing) to the catalog,
-- and add Blueberry Cheesecake — clearly a house signature.
update products set image_url = '/img/' || slug || '.jpg' where slug in (
    'almond-tea-cake',
    'classic-baked-cheesecake',
    'lemon-cheese-mousse',
    'biscoff-cheesecake',
    'nutella-cheesecake',
    'belgian-chocolate-truffle',
    'dark-chocolate-mousse',
    'punjabi-chocolate-classic',
    'chocolate-caramel-almond-brittle'
);

-- Blueberry cheesecake: a signature the shop clearly makes (whole + individual).
insert into products (name, slug, description, price_inr, image_url, category_id, is_eggless_available, is_featured)
select
    'Blueberry Cheesecake',
    'blueberry-cheesecake',
    'Baked cheesecake under a glossy wild-blueberry compote, finished with a cream-piped edge. 500 g.',
    950,
    '/img/blueberry-cheesecake.jpg',
    (select id from categories where slug = 'cheesecakes'),
    true,
    true
where not exists (select 1 from products where slug = 'blueberry-cheesecake');

-- Feature the items we have beautiful photography for; unfeature the rest so the
-- home page leads with real images.
update products set is_featured = true where slug in (
    'punjabi-chocolate-classic', 'blueberry-cheesecake', 'biscoff-cheesecake',
    'lemon-cheese-mousse', 'dark-chocolate-mousse', 'chocolate-caramel-almond-brittle'
);
update products set is_featured = false where slug in (
    'fresh-fruit-chantilly', 'rasmalai-cake', 'tiramisu-cup',
    'fudgy-walnut-brownie-box', 'dense-chocolate-loaf'
);
